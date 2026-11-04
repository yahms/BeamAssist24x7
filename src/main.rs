//! BeamAssist — voice and text commands for BeamNG.drive.
//! Usage:  beam-assist.exe                 voice (if enabled) + typed commands
//!         beam-assist.exe lock rear diff  run one command and exit
//!         beam-assist.exe --models        download/verify all model files

mod ai;
mod app;
mod catalog;
mod config;
mod env;
mod gpu;
mod help;
mod intents;
mod lua;
mod mcp;
mod models;
mod paint;
mod places;
mod router;
mod voice;

use anyhow::{Context, Result};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        eprintln!("Press Enter to exit.");
        let _ = std::io::stdin().lock().read_line(&mut String::new());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let exe = std::env::current_exe().context("locating exe")?;
    let dir = exe.parent().context("exe has no folder")?.to_path_buf();
    let cfg_path = dir.join("config.toml");
    let (cfg, created) = config::load_or_create(&cfg_path)?;
    println!("BeamAssist {}", env!("CARGO_PKG_VERSION"));
    if created {
        println!("Created default config: {}", cfg_path.display());
    }

    // Model files: show where each one is (full path); `--models` downloads/verifies them now.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a == "--models").unwrap_or(false) {
        models::ensure_all(&cfg.models, &dir)?;
        println!("All models present.");
        return Ok(());
    }
    println!("Models folder: {}", models::display(&models::resolve_dir(&cfg.models, &dir)));
    for (name, path, ok) in models::status(&cfg.models, &dir) {
        println!("  {:<6} {}{}", name, models::display(&path), if ok { "" } else { "  (not downloaded yet)" });
    }

    let mcp = mcp::Mcp::new(&cfg.mcp);
    println!("Connecting to {}", mcp.url());
    let info = loop {
        match mcp.initialize() {
            Ok(i) => break i,
            Err(e) => {
                println!("  game not reachable yet ({:#}); retrying in 3 s", e);
                std::thread::sleep(Duration::from_secs(3));
            }
        }
    };
    println!(
        "Connected: {} {}",
        info["serverInfo"]["name"].as_str().unwrap_or("?"),
        info["serverInfo"]["version"].as_str().unwrap_or("")
    );
    if let Ok(tools) = mcp.list_tools() {
        for needed in ["run_lua", "run_lua_vehicle", "show_message", "spawn", "load_level", "recover_vehicle", "trigger_action"] {
            if !tools.iter().any(|t| t == needed) {
                println!("  warning: game MCP has no '{}' tool", needed);
            }
        }
    }

    let cat = catalog::Catalog::load(&mcp).context("reading vehicle and map lists from the game")?;
    println!("{} vehicles, {} maps", cat.models.len(), cat.levels.len());

    let models_cfg = cfg.models.clone();
    let voice_cfg = cfg.voice.clone();
    let router_cfg = cfg.router.clone();
    let mut app = app::App::new(mcp, cfg, cat);
    // The game sometimes leaves the main menu over a loaded map.
    if let Ok(true) = app.ensure_play() {
        println!("Closed the game menu");
    }
    if !args.is_empty() {
        let line = args.join(" ");
        if router_cfg.enabled && intents::parse(&line) == intents::Intent::Unknown {
            match load_router(&models_cfg, &dir, &router_cfg) {
                Ok(r) => app.router = Some(r),
                Err(e) => println!("Router not available: {:#}", e),
            }
        }
        app.handle(&line);
        return Ok(());
    }

    let (tx, rx) = std::sync::mpsc::channel::<voice::Input>();

    // ---- stage B router: loads in the background (a few seconds); commands work meanwhile
    if router_cfg.enabled {
        let (tx, models_cfg, dir, rc) = (tx.clone(), models_cfg.clone(), dir.clone(), router_cfg.clone());
        std::thread::spawn(move || match load_router(&models_cfg, &dir, &rc) {
            Ok(r) => {
                let _ = tx.send(voice::Input::Router(Box::new(r)));
            }
            Err(e) => println!("\nRouter not available, using the rule parser only: {:#}", e),
        });
    }

    // ---- voice
    let mut voice_on = false;
    if voice_cfg.enabled {
        match models::ensure_some(&models_cfg, &dir, &["stt", "vad"]) {
            Ok(paths) => {
                let get = |n: &str| paths.iter().find(|(k, _)| k == n).map(|(_, p)| p.clone()).unwrap_or_default();
                println!("Loading speech model {}", models::display(&get("stt")));
                println!("Loading VAD model    {}", models::display(&get("vad")));
                voice::start(
                    voice::Setup { stt: get("stt"), vad: get("vad"), prompt: vocab_prompt(&app, &voice_cfg), cfg: voice_cfg.clone() },
                    tx.clone(),
                );
                voice_on = true;
            }
            Err(e) => println!("Voice disabled: {:#}", e),
        }
    }
    let how = if voice_on {
        format!("voice ready: say \"{}, …\"", voice_cfg.wake_phrases.first().cloned().unwrap_or_default())
    } else {
        "connected (text mode)".to_string()
    };
    app.say("BeamAssist", &how, "applied", Some(5.0));

    // ---- typed commands on their own thread
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            loop {
                let mut line = String::new();
                match stdin.lock().read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(voice::Input::Quit);
                        break;
                    }
                    Ok(_) => {
                        if tx.send(voice::Input::Text(line.trim().to_string())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    println!("Type a command, e.g. 'lock rear diff', 'high beams', 'spawn bruckell legran'. 'help' lists everything; 'quit' exits.");
    prompt();
    let mut armed_until: Option<Instant> = None;
    let window = Duration::from_secs_f64(voice_cfg.listen_window.max(1.0));
    while let Ok(input) = rx.recv() {
        match input {
            voice::Input::Quit => break,
            voice::Input::Router(r) => {
                println!("\nRouter ready ({} example phrasings)", r.len());
                app.router = Some(*r);
                prompt();
            }
            voice::Input::Text(l) => {
                if l.is_empty() {
                } else if l.eq_ignore_ascii_case("quit") || l.eq_ignore_ascii_case("exit") {
                    break;
                } else if l.eq_ignore_ascii_case("models") {
                    for (name, path, ok) in models::status(&models_cfg, &dir) {
                        println!("  {:<6} {}{}", name, models::display(&path), if ok { "" } else { "  (not downloaded yet)" });
                    }
                } else {
                    app.handle(&l);
                }
                prompt();
            }
            voice::Input::Voice(text) => {
                let armed = armed_until.map(|t| Instant::now() < t).unwrap_or(false);
                let known = |s: &str| intents::parse(s) != intents::Intent::Unknown;
                match voice::gate(&text, &voice_cfg, armed, &known) {
                    voice::Gate::Armed => {
                        armed_until = Some(Instant::now() + window);
                        println!("  (listening…)");
                        app.say("Listening", "go ahead", "already", Some(voice_cfg.listen_window));
                    }
                    voice::Gate::Run(cmd) => {
                        armed_until = None;
                        println!("> {}  (voice)", cmd);
                        app.handle(&cmd);
                        prompt();
                    }
                    voice::Gate::Ignore => {
                        if voice_cfg.show_heard {
                            println!("  (ignored: no wake phrase)");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn load_router(models: &config::ModelsCfg, dir: &std::path::Path, rc: &config::RouterCfg) -> Result<router::Router> {
    let paths = models::ensure_some(models, dir, &["embed", "embed_config", "embed_tokenizer"])?;
    let get = |n: &str| paths.iter().find(|(k, _)| k == n).map(|(_, p)| p.clone()).unwrap_or_default();
    let t = Instant::now();
    let emb = router::Embedder::load(&get("embed"), &get("embed_config"), &get("embed_tokenizer"))?;
    let r = router::Router::new(emb, rc.threshold, rc.margin)?;
    println!("Router model {} loaded in {:.1} s", models::display(&get("embed")), t.elapsed().as_secs_f64());
    Ok(r)
}

fn prompt() {
    print!("> ");
    std::io::stdout().flush().ok();
}

/// Words Whisper should expect: wake phrases, vehicle and map names, drivetrain terms.
/// Whisper's prompt is limited (~220 tokens), so keep it short.
fn vocab_prompt(app: &app::App, v: &config::VoiceCfg) -> String {
    let mut words: Vec<String> = v.wake_phrases.iter().map(|w| {
        let mut c = w.chars();
        match c.next() { Some(f) => f.to_uppercase().collect::<String>() + c.as_str(), None => String::new() }
    }).collect();
    words.extend(
        ["BeamNG", "diff lock", "rangebox", "transfer case", "4WD", "high beams", "hazards", "traction control", "respawn"]
            .iter()
            .map(|s| s.to_string()),
    );
    for m in &app.cat.models {
        if matches!(m.kind.as_str(), "Car" | "Truck") && !words.contains(&m.name) {
            words.push(m.name.clone());
        }
    }
    let mut brands: Vec<String> = app.cat.models.iter().filter(|m| !m.brand.is_empty()).map(|m| m.brand.clone()).collect();
    brands.sort();
    brands.dedup();
    words.extend(brands);
    for l in &app.cat.levels {
        words.push(l.title.clone());
    }
    let mut out = String::new();
    for w in words {
        if out.len() + w.len() + 2 > 700 {
            break;
        }
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(&w);
    }
    out.push('.');
    out
}
