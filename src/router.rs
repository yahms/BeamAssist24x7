//! Stage B: embedding router. When the rule parser doesn't recognise a request, the router
//! compares it with a bank of example phrasings (bge-small-en-v1.5, run on the CPU with Candle)
//! and rewrites it into a canonical command the parser does understand:
//!   "dip the headlights"          -> "low beams"
//!   "get me over to chinatown"    -> "take me to chinatown"
//!   "is the engine cooking"       -> "temperatures"
//! Slots (places, car models, colours, numbers) are carried over from the original words.

use anyhow::{anyhow, Result};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use std::path::Path;
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

pub struct Embedder {
    model: BertModel,
    tok: Tokenizer,
    device: Device,
}

impl Embedder {
    pub fn load(weights: &Path, config: &Path, tokenizer: &Path) -> Result<Self> {
        let device = Device::Cpu;
        let cfg: BertConfig = serde_json::from_str(&std::fs::read_to_string(config)?)?;
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)? };
        let model = BertModel::load(vb, &cfg)?;
        let mut tok = Tokenizer::from_file(tokenizer).map_err(|e| anyhow!("tokenizer: {e}"))?;
        tok.with_padding(Some(PaddingParams::default()));
        tok.with_truncation(Some(TruncationParams { max_length: 64, ..Default::default() }))
            .map_err(|e| anyhow!("tokenizer: {e}"))?;
        Ok(Embedder { model, tok, device })
    }

    /// L2-normalised sentence embeddings (CLS pooling, as bge recommends).
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(32) {
            let enc = self.tok.encode_batch(chunk.to_vec(), true).map_err(|e| anyhow!("tokenize: {e}"))?;
            let ids: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_ids(), &self.device)).collect::<candle_core::Result<_>>()?;
            let mask: Vec<Tensor> =
                enc.iter().map(|e| Tensor::new(e.get_attention_mask(), &self.device)).collect::<candle_core::Result<_>>()?;
            let ids = Tensor::stack(&ids, 0)?;
            let mask = Tensor::stack(&mask, 0)?;
            let types = ids.zeros_like()?;
            let hidden = self.model.forward(&ids, &types, Some(&mask))?; // [batch, seq, 384]
            let cls = hidden.i((.., 0))?; // [batch, 384]
            let norm = cls.sqr()?.sum_keepdim(1)?.sqrt()?;
            let cls = cls.broadcast_div(&norm)?;
            out.extend(cls.to_vec2::<f32>()?);
        }
        Ok(out)
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// (canonical command, carries slots?, example phrasings)
/// The canonical text must be something the rule parser understands. With slots, the user's
/// own content words (place, model, colour, number) are appended to it.
pub const BANK: &[(&str, bool, &[&str])] = &[
    // --- lights
    ("high beams", false, &["more light", "brighter headlights", "turn on the brights", "full beam", "main beam please", "brights on", "light up the road properly", "i can't see a thing out here", "flick the high beams on"]),
    ("low beams", false, &["dip the headlights", "dim the lights", "normal headlights", "lights on please", "turn the headlamps on", "dip the beam", "bring the brights down"]),
    ("headlights off", false, &["kill the lights", "turn the lights off", "lights out", "switch off the headlamps", "no more lights"]),
    ("fog lights on", false, &["foglamps on", "turn on the fog lamps", "put the fogs on"]),
    ("fog lights off", false, &["foglamps off", "turn off the fog lamps", "fogs off"]),
    ("hazards on", false, &["emergency flashers", "hazard lights on", "put the flashers on", "warning lights on", "turn on the emergency blinkers", "four way flashers on"]),
    ("hazards off", false, &["flashers off", "warning lights off", "stop the emergency blinkers"]),
    ("indicate left", false, &["left blinker", "signal left", "i'm turning left"]),
    ("indicate right", false, &["right blinker", "signal right", "i'm turning right"]),
    ("cancel indicator", false, &["blinker off", "stop signalling", "turn the blinker off"]),
    // --- drivetrain
    ("lock all diffs", false, &["lock everything up", "full lockers", "lock it all up", "engage all the lockers", "lock every differential"]),
    ("unlock all diffs", false, &["open up the diffs", "free up the diffs", "disengage the lockers", "turn the lockers off"]),
    ("lock the rear diff", false, &["lock the back end", "engage the rear locker", "rear locker on"]),
    ("lock the front diff", false, &["engage the front locker", "front locker on"]),
    ("low range", false, &["low gearing for climbing", "crawl mode", "low gears for the hill", "put it in low", "crawler gear", "low box", "go to four low", "low ratio please"]),
    ("high range", false, &["back to high", "high box", "four high", "out of low range"]),
    ("four wheel drive", false, &["engage four by four", "drive all four wheels", "send power to the front too", "all wheel drive on"]),
    ("two wheel drive", false, &["rear wheel drive only", "disengage four by four", "back to two wheel drive"]),
    ("traction control off", false, &["turn off the safety systems", "no assists", "let it slide", "turn off the nannies", "disable stability control", "i want to drift", "turn off the driving aids"]),
    ("traction control on", false, &["safety systems on", "assists on", "turn the nannies back on", "enable stability control", "driving aids on"]),
    // --- gears
    ("park", false, &["put it in park", "shift to park"]),
    ("reverse", false, &["back up", "go backwards", "shift into reverse"]),
    ("neutral", false, &["put it in neutral", "out of gear"]),
    ("drive", false, &["put it in drive", "shift to drive", "go forward"]),
    // --- readouts
    ("temperatures", false, &["is the engine overheating", "how hot is it running", "is the engine cooking", "check the coolant"]),
    ("fuel", false, &["how much petrol have i got", "fuel level", "how much diesel is left", "how much gas is left", "am i running low on fuel", "do i need to fill up", "how full is the tank"]),
    ("speed", false, &["how fast am i going", "what's my speed", "am i speeding"]),
    ("engine", false, &["what are the revs", "how many rpm", "is the engine running"]),
    ("damage", false, &["how bad is the damage", "is the car broken", "what did i break", "how wrecked is it"]),
    ("what can i switch", false, &["what can this car do", "what features does this car have", "what controls are there", "what can i change on this car"]),
    // --- vehicle management
    ("repair", false, &["fix my car", "patch it up", "make it new again", "i broke it", "undo the damage", "heal the car"]),
    ("respawn", false, &["restart from the beginning", "go back to the start", "send me back to the start", "put me back where i started", "start over", "take me back to the spawn"]),
    ("spawn", true, &["i d like to drive a", "let me drive a", "i d like a", "i want to drive a", "can i have a", "swap me into a", "put me in a", "give me a different car a", "change my car to a"]),
    ("list cars", false, &["what cars are there", "what can i drive", "show me the vehicles", "which cars do i have"]),
    ("list maps", false, &["what maps do i have", "which levels are there", "show me the maps"]),
    ("what map is this", false, &["which map am i on", "what level is this", "where am i"]),
    ("change map to", true, &["load up the map", "switch the level to", "i want to play on the map"]),
    ("paint my car", true, &["respray it", "i want my car to be", "change the paint to", "make my car look"]),
    // --- environment
    ("night", false, &["make it night time", "pitch black", "make it dark outside", "midnight please", "make it dark", "turn off the sun", "it should be night time", "lights out for the world"]),
    ("noon", false, &["daylight please", "broad daylight", "make it daytime", "turn on the sun", "make it bright outside", "middle of the day"]),
    ("sunset", false, &["golden hour", "make it evening", "sun going down"]),
    ("sunrise", false, &["early morning", "make it dawn", "sun coming up"]),
    ("light rain", false, &["make it rain", "a bit of drizzle", "start raining"]),
    ("heavy rain", false, &["make it pour", "bucket it down", "torrential rain", "a downpour"]),
    ("no rain", false, &["stop the rain", "dry it up", "turn off the rain"]),
    ("clear skies", false, &["clear up the weather", "get rid of the clouds", "nice weather please", "blue skies"]),
    ("heavy fog", false, &["make it misty", "thick fog", "pea souper"]),
    ("no fog", false, &["clear the fog", "get rid of the mist"]),
    ("stop time", false, &["freeze the clock", "keep it this time of day", "stop the sun moving"]),
    ("slow motion", false, &["slow everything down", "bullet time", "matrix mode"]),
    ("real time", false, &["normal speed", "back to normal speed", "full speed simulation"]),
    ("moon gravity", false, &["low gravity", "make me float", "lunar gravity"]),
    ("normal gravity", false, &["earth gravity", "gravity back to normal"]),
    ("what weather settings can i change", false, &["what can i do with the weather", "which environment options are there"]),
    // --- traffic
    ("spawn traffic", false, &["fill the roads with cars", "i want some company on the road", "add some cars to the roads", "make the roads busy"]),
    ("remove traffic", false, &["clear the roads", "get rid of the other cars", "i want the roads empty", "delete all the ai"]),
    ("spawn police", true, &["call the cops", "send the police", "i want to be chased by police"]),
    // --- game
    ("help", false, &["i don't know what to say", "what are my options", "how does this work", "what are you capable of"]),
    ("close menu", false, &["get rid of this menu", "hide the menu", "exit the menu"]),
    ("pause the game", false, &["wait a moment", "hold on a second", "give me a minute", "freeze the game", "stop everything"]),
    // --- body, engine, boost, trucks
    ("open the doors", false, &["let me out", "pop the doors", "doors open please"]),
    ("close the doors", false, &["shut everything", "doors shut", "close up the car"]),
    ("open the bonnet", false, &["let me see the engine", "pop the hood", "open up the engine bay"]),
    ("open the boot", false, &["open the back", "i need to load the trunk", "pop the trunk"]),
    ("start the engine", false, &["start her up", "turn the key", "get it running", "crank it over", "wake the car up"]),
    ("stop the engine", false, &["switch it off", "turn the motor off", "shut it down"]),
    ("jato on", false, &["fire the jato", "rockets on", "light the rockets", "blast off", "launch me", "rocket boost"]),
    ("jato off", false, &["stop the rockets", "cut the rockets"]),
    ("arm the nitrous", false, &["get the nos ready", "prime the nitro", "nitro on"]),
    ("disarm nitrous", false, &["nitro off", "make the nos safe"]),
    ("engine brake on", false, &["use the engine to slow down", "jake on", "exhaust brake on"]),
    ("engine brake off", false, &["jake off", "exhaust brake off"]),
    ("retarder auto", false, &["let the retarder decide", "automatic retarder"]),
    ("parking brake on", false, &["put the handbrake on", "hold the car still", "secure the vehicle", "pull the handbrake"]),
    ("release the handbrake", false, &["let the brake off", "take the parking brake off", "free the wheels"]),
    // --- places, satnav, autopilot
    ("list places", false, &["where can i go", "what places are there", "show me the locations", "what destinations are on this map"]),
    ("navigate to", true, &["how do i get to", "guide me to", "show me the way to", "plot a route to", "point me towards", "give me directions to"]),
    ("how far is", true, &["how many metres to", "how far away is", "how long to get to", "is it far to", "how many kilometres to", "which way is"]),
    ("navigate to the nearest gas station", false, &["i need fuel", "i'm out of gas", "where can i fill up", "find me some petrol"]),
    ("navigate to the nearest charging station", false, &["i need to charge", "my battery is low", "where can i plug in"]),
    ("cancel satnav", false, &["stop the directions", "i don't need directions anymore", "get rid of the route", "turn off navigation"]),
    ("autopilot on", false, &["drive for me", "you drive", "take the wheel", "drive yourself", "self driving mode"]),
    ("autopilot off", false, &["let me take over", "i ll take over", "i want to drive myself", "hand back control", "i'll drive now", "i'll take it from here", "give me the wheel", "manual control", "stop driving for me"]),
    ("take me to", true, &["chauffeur me to", "get me over to", "drive us over to", "bring me to"]),
    ("teleport to", true, &["beam me over to", "teleport me to", "beam me to", "put me at", "move me instantly to", "warp me to"]),
];

/// Words dropped when carrying slots over: function words, request verbs.
const STOP: &[&str] = &[
    "i", "im", "me", "my", "mine", "we", "us", "our", "you", "your", "it", "its", "the", "a", "an", "to", "for", "of", "on",
    "in", "at", "by", "and", "or", "but", "so", "please", "can", "could", "would", "will", "should", "want", "wanna", "like",
    "need", "get", "got", "let", "lets", "make", "put", "take", "give", "show", "tell", "go", "be", "is", "are", "was",
    "do", "does", "some", "now", "just", "then", "there", "here", "this", "that", "these", "those", "up", "over", "into",
    "onto", "towards", "toward", "way", "how", "what", "which", "where", "hey", "ok", "okay", "right", "quick", "quickly",
    "really", "very", "bit", "little", "new", "different", "another", "drive", "driving", "car", "vehicle", "far", "long",
    "turn", "look", "looks", "cool", "nice", "d", "ll", "ve", "s", "t",
];

fn words(s: &str) -> Vec<String> {
    let mut t = String::new();
    for ch in s.to_lowercase().chars() {
        t.push(if ch.is_alphanumeric() || ch == '%' { ch } else { ' ' });
    }
    t.split_whitespace().map(String::from).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    /// (rewritten command, score, the example it matched)
    Command(String, f32, String),
    /// several commands about equally close: (canonical, score)
    Ambiguous(Vec<(String, f32)>),
    /// nothing close enough (best score)
    None(f32),
}

/// On/off pairs the embedding can't tell apart well: (canonical, group, +1 on / -1 off)
const POLARITY: &[(&str, &str, i8)] = &[
    ("high beams", "lights", 1), ("low beams", "lights", 1), ("headlights off", "lights", -1),
    ("fog lights on", "fog", 1), ("fog lights off", "fog", -1),
    ("hazards on", "hazard", 1), ("hazards off", "hazard", -1),
    ("lock all diffs", "diffs", 1), ("unlock all diffs", "diffs", -1),
    ("four wheel drive", "4wd", 1), ("two wheel drive", "4wd", -1),
    ("traction control on", "tc", 1), ("traction control off", "tc", -1),
    ("start the engine", "engine", 1), ("stop the engine", "engine", -1),
    ("jato on", "jato", 1), ("jato off", "jato", -1),
    ("arm the nitrous", "nos", 1), ("disarm nitrous", "nos", -1),
    ("engine brake on", "ebrake", 1), ("engine brake off", "ebrake", -1),
    ("parking brake on", "pbrake", 1), ("release the handbrake", "pbrake", -1),
    ("autopilot on", "autopilot", 1), ("autopilot off", "autopilot", -1),
    ("open the doors", "doors", 1), ("close the doors", "doors", -1),
    ("light rain", "rain", 1), ("heavy rain", "rain", 1), ("no rain", "rain", -1),
    ("heavy fog", "fogenv", 1), ("no fog", "fogenv", -1),
    ("spawn traffic", "traffic", 1), ("remove traffic", "traffic", -1),
    ("moon gravity", "gravity", 1), ("normal gravity", "gravity", -1),
];
const ON_CUES: &[&str] = &[
    "on", "start", "enable", "engage", "fire", "light", "launch", "activate", "arm", "open", "more", "add", "up", "go",
    "begin", "ignite", "hit", "pop", "set",
];
const OFF_CUES: &[&str] = &[
    "off", "stop", "disable", "kill", "cut", "release", "disengage", "disarm", "close", "shut", "remove", "no", "without",
    "cancel", "end", "less", "quit", "rid", "delete", "clear",
];

fn polarity(canon: &str) -> Option<(&'static str, i8)> {
    POLARITY.iter().find(|(c, _, _)| *c == canon).map(|(_, g, p)| (*g, *p))
}

/// Word overlap allowing small spelling differences ("headlamps" ~ "headlights" doesn't count;
/// "differentials" ~ "differential" does).
fn similar(a: &str, b: &str) -> bool {
    a == b || (a.len() >= 4 && b.len() >= 4 && (a.starts_with(&b[..4]) && b.starts_with(&a[..4])))
}

pub struct Router {
    emb: Embedder,
    /// (bank index, phrase, embedding)
    examples: Vec<(usize, String, Vec<f32>)>,
    /// content words per bank entry, for the shared-word check
    vocab: Vec<Vec<String>>,
    pub threshold: f32,
    pub margin: f32,
    /// above this score no shared word is needed
    pub sure: f32,
}

impl Router {
    pub fn new(emb: Embedder, threshold: f32, margin: f32) -> Result<Self> {
        let mut phrases: Vec<(usize, String)> = vec![];
        let mut vocab: Vec<Vec<String>> = vec![vec![]; BANK.len()];
        for (i, (canon, _, ex)) in BANK.iter().enumerate() {
            phrases.push((i, canon.to_string()));
            for e in ex.iter() {
                phrases.push((i, e.to_string()));
            }
            for p in std::iter::once(*canon).chain(ex.iter().copied()) {
                for w in words(p) {
                    if !STOP.contains(&w.as_str()) && w.len() > 2 && !vocab[i].contains(&w) {
                        vocab[i].push(w);
                    }
                }
            }
        }
        let refs: Vec<&str> = phrases.iter().map(|(_, p)| p.as_str()).collect();
        let vecs = emb.embed(&refs)?;
        let examples = phrases.into_iter().zip(vecs).map(|((i, p), v)| (i, p, v)).collect();
        Ok(Router { emb, examples, vocab, threshold, margin, sure: 0.87 })
    }

    pub fn len(&self) -> usize {
        self.examples.len()
    }

    /// Best score per bank entry, highest first.
    pub fn scores(&self, text: &str) -> Result<Vec<(usize, f32, String)>> {
        let q = self.emb.embed(&[text])?.remove(0);
        let mut best: Vec<(usize, f32, String)> = vec![];
        for (i, p, v) in &self.examples {
            let s = dot(&q, v);
            match best.iter_mut().find(|(j, _, _)| j == i) {
                Some(b) if s > b.1 => {
                    b.1 = s;
                    b.2 = p.clone();
                }
                Some(_) => {}
                None => best.push((*i, s, p.clone())),
            }
        }
        best.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(best)
    }

    fn shares_word(&self, i: usize, text: &[String]) -> bool {
        text.iter()
            .filter(|w| !STOP.contains(&w.as_str()) && w.len() > 2)
            .any(|w| self.vocab[i].iter().any(|v| similar(v, w)))
    }

    fn with_slots(&self, i: usize, example: &str, text: &[String]) -> Option<String> {
        let mut used: Vec<String> = words(example);
        used.extend(words(BANK[i].0));
        let rest: Vec<String> = text.iter().filter(|w| !STOP.contains(&w.as_str()) && !used.contains(w)).cloned().collect();
        if rest.is_empty() {
            // "spawn police" works without a count; "plot a route to" needs a destination
            return if BANK[i].0 == "spawn police" { Some(BANK[i].0.to_string()) } else { None };
        }
        Some(format!("{} {}", BANK[i].0, rest.join(" ")))
    }

    pub fn route(&self, text: &str) -> Result<Route> {
        let tw = words(text);
        let joined = format!(" {} ", tw.join(" "));
        // 1. A slot command spoken with one of its example phrasings word for word
        //    ("guide me to chinatown"): the place/model words would only blur the embedding.
        let mut lit: Option<(usize, &str)> = None;
        for (i, (_, slots, ex)) in BANK.iter().enumerate() {
            if !*slots {
                continue;
            }
            for e in ex.iter() {
                let ew = words(e).join(" ");
                if joined.contains(&format!(" {} ", ew)) && lit.map(|(_, l)| ew.len() > l.len()).unwrap_or(true) {
                    lit = Some((i, e));
                }
            }
        }
        if let Some((i, e)) = lit {
            if let Some(cmd) = self.with_slots(i, e, &tw) {
                return Ok(Route::Command(cmd, 1.0, e.to_string()));
            }
        }

        // 2. Embedding similarity.
        let best = self.scores(text)?;
        let Some(first) = best.first() else { return Ok(Route::None(0.0)) };
        let (mut i, top) = (first.0, first.1);
        let mut example = first.2.clone();
        if top < self.threshold {
            return Ok(Route::None(top));
        }
        if top < self.sure && !self.shares_word(i, &tw) {
            return Ok(Route::None(top));
        }
        let close: Vec<&(usize, f32, String)> = best.iter().take_while(|(_, s, _)| top - s < self.margin).collect();
        // On/off twins: settle by the cue words actually spoken.
        if let Some((group, _)) = polarity(BANK[i].0) {
            let on = tw.iter().any(|w| ON_CUES.contains(&w.as_str()));
            let off = tw.iter().any(|w| OFF_CUES.contains(&w.as_str()));
            let mut want: Option<i8> = match (on, off) {
                (true, false) => Some(1),
                (false, true) => Some(-1),
                _ => None,
            };
            if want.is_none() {
                // No cue spoken, but the matched example has one ("disengage four by four"):
                // the request didn't say it, so take the other side.
                let ew = words(&example);
                if ew.iter().any(|w| OFF_CUES.contains(&w.as_str())) {
                    want = Some(1);
                } else if ew.iter().any(|w| ON_CUES.contains(&w.as_str())) && polarity(BANK[i].0).map(|(_, p)| p) == Some(-1) {
                    want = Some(-1);
                }
            }
            if let Some(want) = want {
                if let Some(b) = best
                    .iter()
                    .take(6)
                    .find(|(j, s, _)| polarity(BANK[*j].0) == Some((group, want)) && top - s < 0.1)
                {
                    i = b.0;
                    example = b.2.clone();
                }
            }
            let others: Vec<&&(usize, f32, String)> =
                close.iter().filter(|(j, _, _)| polarity(BANK[*j].0).map(|(g, _)| g) != Some(group)).collect();
            if !others.is_empty() {
                let mut v = vec![(BANK[i].0.to_string(), top)];
                v.extend(others.iter().map(|(j, s, _)| (BANK[*j].0.to_string(), *s)));
                return Ok(Route::Ambiguous(v));
            }
        } else if close.len() > 1 {
            // Close runners-up are a genuine tie (stage C will settle these).
            return Ok(Route::Ambiguous(close.iter().map(|(j, s, _)| (BANK[*j].0.to_string(), *s)).collect()));
        }
        let (canon, slots, _) = BANK[i];
        if slots {
            return Ok(match self.with_slots(i, &example, &tw) {
                Some(cmd) => Route::Command(cmd, top, example),
                None => Route::None(top),
            });
        }
        Ok(Route::Command(canon.to_string(), top, example))
    }
}
