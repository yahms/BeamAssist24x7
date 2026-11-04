//! Voice input: microphone -> level-based endpointing -> Silero VAD check -> Whisper.
//! Runs on its own thread and sends transcripts to the main loop; the wake gate lives here
//! too (pure functions) so main can decide what to run.
//!
//! APIs (checked on docs.rs 2026-11-03): whisper-rs 0.16, cpal 0.18.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext,
    WhisperVadContextParams, WhisperVadParams,
};

use crate::config::VoiceCfg;

/// Everything the main loop receives.
pub enum Input {
    Text(String),
    Voice(String),
    Quit,
}

const RATE: usize = 16_000;
const FRAME: usize = RATE * 30 / 1000; // 30 ms

pub struct Setup {
    pub stt: PathBuf,
    pub vad: PathBuf,
    pub prompt: String,
    pub cfg: VoiceCfg,
}

/// Start the voice thread. Errors after start-up are printed and stop voice only.
pub fn start(setup: Setup, tx: Sender<Input>) {
    std::thread::spawn(move || {
        if let Err(e) = run(setup, tx) {
            eprintln!("Voice stopped: {:#}", e);
        }
    });
}

fn mono<T: Copy>(data: &[T], ch: usize, conv: fn(T) -> f32) -> Vec<f32> {
    let ch = ch.max(1);
    data.chunks(ch).map(|f| f.iter().map(|&x| conv(x)).sum::<f32>() / f.len() as f32).collect()
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, tx: Sender<Vec<f32>>, conv: fn(T) -> f32) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + Copy + Send + 'static,
{
    let ch = config.channels as usize;
    let stream = device.build_input_stream(
        config.clone(),
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let _ = tx.send(mono(data, ch, conv));
        },
        |e| eprintln!("microphone error: {}", e),
        None,
    )?;
    Ok(stream)
}

/// Device rate -> 16 kHz: box-filter average over the decimation window, then sample.
struct Resampler {
    ratio: f64,
    pos: f64,
    buf: Vec<f32>,
}

impl Resampler {
    fn new(rate: u32) -> Self {
        Resampler { ratio: rate as f64 / RATE as f64, pos: 0.0, buf: vec![] }
    }
    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if (self.ratio - 1.0).abs() < 1e-6 {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        let w = (self.ratio.round() as usize).max(1);
        while (self.pos as usize) + w < self.buf.len() {
            let i = self.pos as usize;
            out.push(self.buf[i..i + w].iter().sum::<f32>() / w as f32);
            self.pos += self.ratio;
        }
        let drop = (self.pos as usize).min(self.buf.len());
        self.buf.drain(..drop);
        self.pos -= drop as f64;
    }
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn is_noise_text(t: &str) -> bool {
    let l = t.trim().to_lowercase();
    l.is_empty()
        || (l.starts_with('[') && l.ends_with(']'))
        || (l.starts_with('(') && l.ends_with(')'))
        || ["thank you.", "thanks for watching!", "thank you for watching.", "you", "you.", "bye.", "."].contains(&l.as_str())
}

fn run(setup: Setup, tx: Sender<Input>) -> Result<()> {
    let cfg = setup.cfg;

    // --- models: pick GPU or CPU, fall back to CPU if the GPU fails to start
    let size_mb = std::fs::metadata(&setup.stt).map(|m| m.len() / 1_048_576).unwrap_or(150);
    let need_mb = size_mb * 2 + 300; // weights + compute buffers, rough
    let device = crate::gpu::choose(&cfg.device, cfg.gpu_device, need_mb, cfg.vram_headroom_mb, cfg!(feature = "gpu"));
    let load = |gpu: Option<i32>| {
        let mut cp = WhisperContextParameters::new();
        cp.use_gpu(gpu.is_some());
        if let Some(d) = gpu {
            cp.gpu_device(d);
        }
        WhisperContext::new_with_params(&setup.stt, cp)
    };
    let ctx = match device {
        crate::gpu::Device::Gpu(idx, what) => match load(Some(idx)) {
            Ok(c) => {
                println!("Speech recognition on GPU: {}", what);
                c
            }
            Err(e) => {
                println!("Speech recognition: GPU failed to start ({:?}); using CPU", e);
                load(None).map_err(|e| anyhow!("loading speech model {}: {:?}", setup.stt.display(), e))?
            }
        },
        crate::gpu::Device::Cpu(why) => {
            println!("Speech recognition on CPU ({} threads): {}", cfg.threads, why);
            load(None).map_err(|e| anyhow!("loading speech model {}: {:?}", setup.stt.display(), e))?
        }
    };
    let mut state = ctx.create_state().map_err(|e| anyhow!("whisper state: {:?}", e))?;
    let mut vcp = WhisperVadContextParams::new();
    vcp.set_n_threads(2);
    vcp.set_use_gpu(false);
    let vad_path = setup.vad.to_string_lossy().to_string();
    let mut vad = WhisperVadContext::new(&vad_path, vcp).map_err(|e| anyhow!("loading VAD model {}: {:?}", vad_path, e))?;
    let mut vp = WhisperVadParams::new();
    vp.set_threshold(cfg.vad_threshold);

    // --- microphone
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| anyhow!("no microphone found"))?;
    let sup = device.default_input_config()?;
    let rate: u32 = sup.sample_rate();
    let config = sup.config();
    let (atx, arx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = channel();
    let stream = match sup.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, atx, |x| x)?,
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, atx, |x| x as f32 / 32768.0)?,
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, atx, |x| (x as f32 - 32768.0) / 32768.0)?,
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, atx, |x| x as f32 / 2_147_483_648.0)?,
        other => return Err(anyhow!("unsupported microphone sample format {:?}", other)),
    };
    stream.play()?;
    println!(
        "Voice: listening on the default microphone ({} Hz, {} ch), mode '{}', wake: {}",
        rate,
        config.channels,
        cfg.mode,
        cfg.wake_phrases.join(" / ")
    );

    // --- endpointing state
    let mut rs = Resampler::new(rate);
    let mut pcm: Vec<f32> = vec![];
    let pre_max = RATE * 400 / 1000;
    let mut pre: VecDeque<f32> = VecDeque::with_capacity(pre_max + FRAME);
    let mut cur: Vec<f32> = vec![];
    let mut active = false;
    let mut speech_frames = 0u32;
    let mut silence_ms = 0u32;
    let mut noise = 0.0f32;
    let max_len = (cfg.max_utterance_s * RATE as f32) as usize;

    loop {
        let chunk = match arx.recv_timeout(Duration::from_secs(5)) {
            Ok(c) => c,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err(anyhow!("microphone stream ended")),
        };
        rs.push(&chunk, &mut pcm);
        while pcm.len() >= FRAME {
            let frame: Vec<f32> = pcm.drain(..FRAME).collect();
            let level = rms(&frame);
            if !active && (noise == 0.0 || level < noise * 2.0) {
                noise = if noise == 0.0 { level } else { noise * 0.95 + level * 0.05 };
            }
            let speech = level > (noise * cfg.level_ratio).max(cfg.min_level);
            if !active {
                pre.extend(frame.iter());
                while pre.len() > pre_max {
                    pre.pop_front();
                }
                speech_frames = if speech { speech_frames + 1 } else { 0 };
                if speech_frames >= 3 {
                    active = true;
                    silence_ms = 0;
                    cur = pre.drain(..).collect();
                }
                continue;
            }
            cur.extend_from_slice(&frame);
            silence_ms = if speech { 0 } else { silence_ms + 30 };
            if silence_ms >= cfg.end_silence_ms || cur.len() >= max_len {
                active = false;
                speech_frames = 0;
                let audio = std::mem::take(&mut cur);
                if audio.len() < RATE * 4 / 10 {
                    continue; // under 0.4 s
                }
                // Silero confirms there is speech (avoids Whisper "hallucinating" on noise).
                match vad.segments_from_samples(vp, &audio) {
                    Ok(segs) if segs.num_segments() == 0 => continue,
                    Ok(_) => {}
                    Err(e) => eprintln!("VAD error: {:?}", e),
                }
                let t0 = Instant::now();
                let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                p.set_n_threads(cfg.threads);
                p.set_language(Some("en"));
                p.set_no_timestamps(true);
                p.set_single_segment(true);
                p.set_no_context(true);
                p.set_suppress_blank(true);
                p.set_suppress_nst(true);
                p.set_temperature(0.0);
                p.set_print_special(false);
                p.set_print_progress(false);
                p.set_print_realtime(false);
                p.set_print_timestamps(false);
                p.set_initial_prompt(&setup.prompt);
                if let Err(e) = state.full(p, &audio) {
                    eprintln!("speech recognition error: {:?}", e);
                    continue;
                }
                let mut text = String::new();
                for seg in state.as_iter() {
                    if let Ok(s) = seg.to_str_lossy() {
                        text.push_str(&s);
                    }
                }
                let text = text.trim().to_string();
                if cfg.show_heard {
                    println!(
                        "  [heard {:.1}s audio in {:.2}s] {}",
                        audio.len() as f32 / RATE as f32,
                        t0.elapsed().as_secs_f32(),
                        if text.is_empty() { "(nothing)" } else { text.as_str() }
                    );
                }
                if !is_noise_text(&text) && tx.send(Input::Voice(text)).is_err() {
                    return Ok(());
                }
            }
        }
    }
}

// ---------------------------------------------------------------- wake gate

fn words_of(s: &str) -> Vec<String> {
    crate::catalog::norm(s).split(' ').filter(|w| !w.is_empty()).map(String::from).collect()
}

/// If the sentence starts with a wake phrase (fuzzy, since Whisper mishears names),
/// return the rest of the sentence.
pub fn strip_wake(text: &str, phrases: &[String]) -> Option<String> {
    let words = words_of(text);
    for p in phrases {
        let pw = words_of(p);
        if pw.is_empty() {
            continue;
        }
        let target = pw.join("");
        // allow the phrase to be heard as one word more or less ("hey beam" -> "hey bean", "a beam", "heybeam")
        for n in [pw.len(), pw.len() + 1, pw.len().saturating_sub(1)] {
            if n == 0 || n > words.len() {
                continue;
            }
            let heard = words[..n].join("");
            if strsim::normalized_levenshtein(&heard, &target) >= 0.75 {
                return Some(words[n..].join(" "));
            }
        }
    }
    None
}

pub enum Gate {
    /// run this command text
    Run(String),
    /// wake phrase alone: listen for the next sentence
    Armed,
    /// not for us
    Ignore,
}

/// Decide whether a transcript is a command. `armed` = a bare wake phrase was heard recently.
/// `known` tells whether the rule parser recognises the text.
pub fn gate(text: &str, cfg: &VoiceCfg, armed: bool, known: &dyn Fn(&str) -> bool) -> Gate {
    if let Some(rest) = strip_wake(text, &cfg.wake_phrases) {
        return if rest.trim().is_empty() { Gate::Armed } else { Gate::Run(rest) };
    }
    if armed {
        return Gate::Run(text.to_string());
    }
    let n_words = words_of(text).len();
    match cfg.mode.as_str() {
        "open" if known(text) => Gate::Run(text.to_string()),
        "both" if known(text) && n_words <= 8 => Gate::Run(text.to_string()),
        _ => Gate::Ignore,
    }
}
