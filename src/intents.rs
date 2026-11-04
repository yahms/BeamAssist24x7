//! Rule-based intent parser for text mode. This is a stand-in for the embedding/LLM
//! router of the voice milestone: same Intent output, so the executor won't change.

#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    Lights(&'static str), // off | low | high
    Fog(bool),
    Hazard(bool),
    Signal(&'static str), // left | right | off
    DiffLock { scope: &'static str, locked: bool },
    CenterDiff(Option<u32>), // None = auto
    Range(&'static str),     // low | high
    Drive(&'static str),     // 4wd | 2wd
    Traction(bool),
    Gear(String), // P R N D S or a number
    Read(&'static str), // temps | fuel | engine | speed
    Damage,
    ListSwitches,
    Repair,
    Respawn,
    Spawn { words: Vec<String>, add: bool, ai: bool },
    ListConfigs(Vec<String>),
    ListCars,
    ListMaps,
    CurrentMap,
    LoadMap(Vec<String>),
    CloseMenu,
    Paint(Vec<String>),
    Env(Vec<String>),
    Help(Option<&'static str>),
    Traffic(Vec<String>),
    Ai(Vec<String>),
    Pause,
    /// part: door|doors|hood|trunk; which: ""|all|FL|FR|RL|RR|front|rear|left|right; state: open|close|toggle
    Body { part: &'static str, which: &'static str, state: &'static str },
    Engine(bool), // true = start
    Jato(bool),
    Nitrous(bool),
    EngineBrake { on: bool, level: Option<u32>, percent: Option<u32> },
    Retarder(String), // auto | off | on | percent
    ParkingBrake(bool),
    AirSuspension(&'static str), // raise | lower | stop
    ListPlaces(Vec<String>),
    Navigate(Vec<String>),
    Distance(Vec<String>),
    ClearNav,
    Autopilot(bool),
    DriveTo(Vec<String>),
    Teleport(Vec<String>),
    Unknown,
}

const AI_MODES: &[&str] = &[
    "chase", "chasing", "pursue", "hunt", "ram", "follow", "following", "tail", "flee", "escape", "wander", "roam",
    "autopilot", "aggressive", "aggressively",
];
const VEH_DESC: &[&str] = &[
    "van", "vans", "truck", "trucks", "taxi", "police", "cop", "cops", "pickup", "ute", "bus", "traffic", "everyone",
    "all", "them", "sedan", "wagon", "suv", "cars", "ai",
];
const ENV_WORDS: &[&str] = &[
    "time", "clock", "oclock", "am", "pm", "noon", "midday", "midnight", "sunrise", "dawn", "morning", "afternoon",
    "evening", "sunset", "dusk", "night", "daytime", "nighttime", "weather", "clear", "sunny", "cloud", "clouds",
    "cloudy", "overcast", "fog", "foggy", "mist", "misty", "wind", "windy", "breeze", "calm", "rain", "raining", "rainy",
    "storm", "stormy", "drizzle", "gravity", "environment",
];

/// Same mapping as help::category_of (kept here so this module stays standalone-testable).
fn help_category(w: &str) -> Option<&'static str> {
    Some(match w {
        "vehicle" | "driving" | "drivetrain" | "light" | "diff" | "gear" | "gears" | "paint" | "control" => "vehicle",
        "spawn" | "spawning" | "cars" | "car" | "config" | "configs" => "spawn",
        "map" | "maps" | "level" | "levels" => "map",
        "weather" | "time" | "environment" | "rain" | "fog" | "gravity" => "env",
        "traffic" | "ai" | "police" | "cop" | "cops" => "ai",
        "game" | "menu" | "ui" | "pause" => "game",
        "truck" | "trucks" | "brake" | "brakes" | "retarder" | "suspension" => "truck",
        "satnav" | "nav" | "navigation" | "gps" | "places" | "locations" | "destinations" | "autopilot" | "teleport" => "nav",
        _ => return None,
    })
}

fn ai_like(t: &Text) -> bool {
    t.any(AI_MODES)
        || t.any_phrase(&["run away", "drive normally", "drive randomly", "pull over", "drive like traffic", "drive calmly", "drive carefully"])
        || (t.any(&["stop", "halt", "park", "calm", "careful", "slow", "slowly"])
            && (t.any(VEH_DESC) || t.words.iter().any(|w| COLOURS.contains(&w.as_str()))))
}

fn env_like(t: &Text) -> bool {
    let lamp = t.any(&["light", "headlight", "headlights", "beam", "lamp", "lamps"])
        && t.any(&["on", "off", "high", "low", "full", "dipped"]);
    if lamp || (t.has("fog") && t.any(&["light", "lamp", "lamps"])) {
        return false;
    }
    t.any(ENV_WORDS)
        || (t.any(&["temperature", "temp"]) && t.any(&["air", "outside", "ambient", "weather"]))
        || (t.has("slow") && t.has("motion"))
        || (t.any(&["sim", "simulation", "game"]) && t.has("speed"))
        || (t.any(&["half", "quarter", "normal", "real"]) && t.any(&["speed", "time"]) && !t.has("past"))
}

const COLOURS: &[&str] = &[
    "black", "white", "grey", "gray", "silver", "red", "maroon", "burgundy", "crimson", "orange", "yellow", "gold",
    "golden", "green", "cyan", "teal", "turquoise", "aqua", "blue", "navy", "purple", "violet", "pink", "magenta",
    "brown", "tan", "beige", "bronze",
];

struct Text {
    norm: String,
    words: Vec<String>,
}

impl Text {
    fn new(s: &str) -> Self {
        let mut t = String::new();
        for ch in s.to_lowercase().chars() {
            t.push(if ch.is_alphanumeric() || ch == '%' { ch } else { ' ' });
        }
        let words: Vec<String> = t
            .split_whitespace()
            .map(|w| {
                let canon = match w {
                    "centre" => "center",
                    "diffs" | "differential" | "differentials" | "locker" | "lockers" => "diff",
                    "lights" => "light",
                    "beams" => "beam",
                    "hazards" => "hazard",
                    "indicators" | "indicate" | "blinker" | "blinkers" | "signals" => "indicator",
                    other => other,
                };
                canon.to_string()
            })
            .collect();
        let norm = format!(" {} ", words.join(" "));
        Text { norm, words }
    }
    fn has(&self, w: &str) -> bool {
        self.words.iter().any(|x| x == w)
    }
    fn any(&self, ws: &[&str]) -> bool {
        ws.iter().any(|w| self.has(w))
    }
    fn phrase(&self, p: &str) -> bool {
        self.norm.contains(&format!(" {} ", p))
    }
    fn any_phrase(&self, ps: &[&str]) -> bool {
        ps.iter().any(|p| self.phrase(p))
    }
    fn after(&self, ws: &[&str]) -> Vec<String> {
        match self.words.iter().position(|x| ws.contains(&x.as_str())) {
            Some(i) => self.words[i + 1..].to_vec(),
            None => vec![],
        }
    }
    fn number(&self) -> Option<u32> {
        for w in &self.words {
            if let Ok(n) = w.trim_end_matches('%').parse::<u32>() {
                return Some(n);
            }
            let n = match w.as_str() {
                "zero" | "none" => Some(0),
                "twenty" => Some(20),
                "quarter" | "twentyfive" => Some(25),
                "thirty" => Some(30),
                "forty" => Some(40),
                "fifty" | "half" => Some(50),
                "sixty" => Some(60),
                "seventy" => Some(70),
                "eighty" => Some(80),
                "ninety" => Some(90),
                "hundred" | "full" | "fully" => Some(100),
                _ => None,
            };
            if n.is_some() {
                return n;
            }
        }
        None
    }
}

const OFF: &[&str] = &["off", "disable", "disabled", "deactivate", "cancel", "stop", "kill", "no"];
const UNLOCK: &[&str] = &["unlock", "unlocked", "open", "off", "disengage", "release", "free", "disable"];
const FILLER: &[&str] = &[
    "a", "an", "the", "new", "car", "vehicle", "please", "another", "extra", "additional", "second", "with",
    "to", "me", "in", "my", "for", "of", "list", "show", "what", "which", "available", "all", "config",
    "configs", "configuration", "configurations", "variants", "versions", "are", "is", "there", "map", "level",
    "load", "change", "switch", "go", "open", "take", "spawn", "replace", "swap", "add", "get", "give", "bring",
];

fn strip_fillers(w: &[String]) -> Vec<String> {
    w.iter().filter(|x| !FILLER.contains(&x.as_str())).cloned().collect()
}

fn ordinal(w: &str) -> Option<u32> {
    match w {
        "first" | "1st" | "one" => Some(1),
        "second" | "2nd" | "two" => Some(2),
        "third" | "3rd" | "three" => Some(3),
        "fourth" | "4th" | "four" => Some(4),
        "fifth" | "5th" | "five" => Some(5),
        "sixth" | "6th" | "six" => Some(6),
        "seventh" | "7th" | "seven" => Some(7),
        "eighth" | "8th" | "eight" => Some(8),
        _ => w.parse().ok(),
    }
}

const MAP_WORDS: &[&str] = &["map", "maps", "level", "levels"];
const NAV_WORDS: &[&str] = &["satnav", "sat", "nav", "navigation", "gps", "route", "directions", "navigate", "navigator"];

/// Words after the first of `ws`, or all words if none.
fn tail(t: &Text, ws: &[&str]) -> Vec<String> {
    let a = t.after(ws);
    if a.is_empty() { t.words.clone() } else { a }
}

fn places_intent(t: &Text, listy: bool, off: bool) -> Option<Intent> {
    if t.any(MAP_WORDS) && !t.any(&["places", "locations", "destinations", "spots", "on"]) {
        return None;
    }
    let nav = t.any(NAV_WORDS);
    if nav && t.any(&["clear", "cancel", "stop", "remove", "off", "delete", "end", "reset"]) {
        return Some(Intent::ClearNav);
    }
    if t.any(&["teleport", "warp", "beam"]) && t.has("to") || t.phrase("spawn at") || t.phrase("spawn me at") || t.phrase("jump to") {
        return Some(Intent::Teleport(tail(t, &["to", "at"])));
    }
    if t.any_phrase(&["take me to", "drive me to", "drive to", "autopilot to", "take us to", "drive us to", "auto drive to"]) {
        return Some(Intent::DriveTo(tail(t, &["to"])));
    }
    if t.has("autopilot") || t.any(&["autodrive", "selfdriving"]) || t.any_phrase(&["auto drive", "self driving", "self drive"]) {
        // "make the white van autopilot" etc. stays with the AI handler
        if !t.any(&["van", "truck", "taxi", "police", "cop", "cops", "pickup", "bus", "traffic", "them"]) {
            return Some(Intent::Autopilot(!off && !t.any(&["disengage", "disable"])));
        }
    }
    if !t.has("wind") && (t.phrase("how far") || t.any(&["distance", "direction", "bearing", "heading"]) || t.phrase("which way")) {
        return Some(Intent::Distance(t.words.clone()));
    }
    if (nav && t.any(&["to", "for"]))
        || t.any_phrase(&["how to get to", "how do i get to", "how do we get to", "where is", "where s", "wheres", "find the nearest", "find the closest", "find a", "find me a", "find me the", "directions to", "way to"])
    {
        return Some(Intent::Navigate(tail(t, &["to", "for", "is", "s", "wheres", "find"])));
    }
    let place_words = ["places", "locations", "destinations", "spots", "stations", "station", "chargers", "pois"];
    if (listy || t.phrase("where can i go") || t.phrase("where can we go")) && (t.any(&place_words) || t.phrase("where can i go") || t.phrase("where can we go")) {
        return Some(Intent::ListPlaces(t.words.clone()));
    }
    None
}

fn feature_intent(t: &Text, off: bool) -> Option<Intent> {
    let brake = t.any(&["brake", "brakes", "braking"]);
    // --- truck brakes
    if (t.has("engine") && brake) || (t.any(&["jake", "jakes", "compression", "exhaust"]) && (brake || t.has("jake"))) {
        let percent = t.words.iter().find_map(|w| w.strip_suffix('%').and_then(|n| n.parse::<u32>().ok()));
        let level = t.after(&["level", "stage", "setting"]).first().and_then(|w| ordinal(w)).or_else(|| {
            t.words.iter().filter_map(|w| w.parse::<u32>().ok()).find(|n| (1..=3).contains(n))
        });
        let percent = percent.or_else(|| t.words.iter().filter_map(|w| w.parse::<u32>().ok()).find(|n| *n > 3));
        return Some(Intent::EngineBrake { on: !off || level.is_some(), level, percent });
    }
    if t.has("retarder") || t.any_phrase(&["hydraulic brake", "retarding brake", "retarder brake"]) {
        let pct = t.words.iter().find_map(|w| w.trim_end_matches('%').parse::<u32>().ok()).or_else(|| t.number());
        let st = if t.has("auto") || t.has("automatic") {
            "auto".to_string()
        } else if off {
            "off".into()
        } else if let Some(p) = pct {
            p.to_string()
        } else {
            "on".into()
        };
        return Some(Intent::Retarder(st));
    }
    if t.any(&["handbrake", "parkingbrake", "ebrake"]) || t.any_phrase(&["parking brake", "park brake", "hand brake", "air brake", "air brakes", "emergency brake"]) {
        return Some(Intent::ParkingBrake(!(off || t.any(&["release", "disengage", "down", "unset"]))));
    }
    if t.has("suspension") || t.phrase("ride height") {
        let st = if t.any(&["raise", "up", "higher", "lift", "increase"]) {
            "raise"
        } else if t.any(&["lower", "down", "drop", "decrease", "kneel"]) {
            "lower"
        } else {
            "stop"
        };
        return Some(Intent::AirSuspension(st));
    }
    // --- boost
    if t.any(&["jato", "rocket", "rockets", "jatos"]) {
        return Some(Intent::Jato(!off));
    }
    if t.any(&["nos", "nitrous", "n2o", "nitro", "nitrox"]) {
        return Some(Intent::Nitrous(!(off || t.has("disarm"))));
    }
    // --- engine start/stop
    let engine_noun = t.any(&["engine", "motor", "ignition"]);
    let start = t.any(&["start", "crank", "on"]) || t.any_phrase(&["fire it up", "fire up", "start it up", "turn it on"]);
    let stop = t.any(&["stop", "off", "kill", "shut", "cut"]);
    if (engine_noun && (start || stop))
        || (t.words.first().map(|w| w == "start").unwrap_or(false) && t.any(&["car", "it", "vehicle", "truck", "up"]))
        || t.any_phrase(&["fire it up", "start it up", "start the car", "start the truck", "turn the car off", "turn the car on", "switch off the car"])
    {
        return Some(Intent::Engine(!stop || t.has("start")));
    }
    // --- doors, bonnet, boot
    let part = if t.any(&["door", "doors"]) {
        Some(if t.has("doors") { "doors" } else { "door" })
    } else if t.any(&["hood", "bonnet"]) {
        Some("hood")
    } else if t.any(&["boot", "trunk", "tailgate", "hatch", "liftgate", "hatchback"]) {
        Some("trunk")
    } else {
        None
    };
    if let Some(mut part) = part {
        let state = if t.any(&["open", "unlatch", "pop"]) {
            "open"
        } else if t.any(&["close", "shut", "latch"]) {
            "close"
        } else {
            "toggle"
        };
        let front = t.has("front");
        let rear = t.any(&["rear", "back"]);
        let left = t.has("left");
        let right = t.has("right");
        let which: &'static str = if t.has("driver") || t.has("drivers") {
            "FL"
        } else if t.has("passenger") && !rear {
            "FR"
        } else {
            match (front, rear, left, right) {
                (true, false, true, false) => "FL",
                (true, false, false, true) => "FR",
                (false, true, true, false) => "RL",
                (false, true, false, true) => "RR",
                (true, false, false, false) => "front",
                (false, true, false, false) => "rear",
                (false, false, true, false) => "left",
                (false, false, false, true) => "right",
                _ if t.any(&["all", "every", "both", "everything"]) || part == "doors" => "all",
                _ => "",
            }
        };
        if part == "doors" && which.len() == 2 {
            part = "door";
        }
        return Some(Intent::Body { part, which, state });
    }
    None
}

pub fn parse(input: &str) -> Intent {
    let t = Text::new(input);
    if t.words.is_empty() {
        return Intent::Unknown;
    }
    let first = t.words[0].as_str();

    // --- help: "what can I ask", "what can you do", "what commands are there", "help with weather"
    if t.has("help")
        || t.any(&["command", "commands"])
        || t.any_phrase(&["can you do", "can i ask", "can i say", "what do you do", "what do you know", "how do i use you"])
    {
        // Specific settings questions stay with their own handler ("what weather settings can I change").
        if !(t.any(&["settings", "options", "variables"]) && !t.any(&["command", "commands", "help"])) {
            let cat = t.words.iter().find_map(|w| help_category(w));
            return Intent::Help(cat);
        }
    }
    let off = t.any(OFF);
    let listy = t.any(&["list", "show", "what", "which", "available", "tell"]);

    // --- menu
    if t.any(&["menu", "pause", "unpause"]) && t.any(&["close", "dismiss", "hide", "exit", "leave", "off", "unpause", "resume"])
        || t.any_phrase(&["back to game", "back to the game", "resume game", "resume the game"])
    {
        return Intent::CloseMenu;
    }
    if t.has("pause") && !t.any(&["unpause", "resume"]) {
        return Intent::Pause;
    }

    // --- paint
    let last_is_colour = t.words.last().map(|w| COLOURS.contains(&w.as_str())).unwrap_or(false);
    if t.any(&["paint", "respray", "repaint", "recolour", "recolor", "colour", "color"]) || (first == "make" && last_is_colour) {
        return Intent::Paint(t.words.clone());
    }

    // --- satnav, places, autopilot, teleport
    if let Some(i) = places_intent(&t, listy, off) {
        return i;
    }

    // --- doors, engine, boost, truck controls
    if let Some(i) = feature_intent(&t, off) {
        return i;
    }

    // --- configs / maps / switches / cars
    if t.any(&["config", "configs", "configuration", "configurations", "variants", "versions"])
        && (listy || t.has("other"))
    {
        let after = t.after(&["for", "of"]);
        let w = if after.is_empty() { strip_fillers(&t.words) } else { strip_fillers(&after) };
        return Intent::ListConfigs(w.into_iter().filter(|x| x != "other").collect());
    }
    if t.any(&["map", "maps", "level", "levels"]) {
        if t.any(&["load", "change", "switch", "go", "open", "take", "travel"]) {
            let after = t.after(&["to"]);
            let w = if after.is_empty() { strip_fillers(&t.words) } else { strip_fillers(&after) };
            return Intent::LoadMap(w);
        }
        if t.any(&["maps", "levels"]) || t.has("list") {
            return Intent::ListMaps;
        }
        return Intent::CurrentMap;
    }
    if t.any(&["switch", "switches", "switchable", "controls", "toggles", "toggle"]) && (listy || t.has("can")) {
        return Intent::ListSwitches;
    }

    // --- traffic / police / AI behaviour / environment
    let ai = ai_like(&t);
    let police_words = t.any(&["police", "cop", "cops", "cruiser", "cruisers"]);
    let spawnish = t.any(&["spawn", "add", "call", "bring", "send", "more", "remove", "clear", "delete", "despawn"]);
    if !ai && (t.has("traffic") || (police_words && spawnish)) {
        return Intent::Traffic(t.words.clone());
    }
    if ai {
        return Intent::Ai(t.words.clone());
    }
    if env_like(&t) {
        return Intent::Env(t.words.clone());
    }

    // --- vehicle management
    if t.any(&["respawn", "reset"]) || t.any_phrase(&["go home", "back home", "back to start", "to the start", "start position"]) {
        return Intent::Respawn;
    }
    if t.any(&["repair", "fix", "restore", "recover", "reload", "heal", "unflip"]) {
        return Intent::Repair;
    }
    let spawn_verb = t.has("spawn")
        || t.any(&["replace", "swap"])
        || first == "add"
        || t.any_phrase(&["change car", "switch car", "change vehicle", "switch vehicle", "give me a", "get me a", "bring me a"]);
    if spawn_verb {
        let ai = t.has("ai");
        let add = first == "add" || ai || t.any(&["another", "extra", "additional"]);
        let verbs = ["spawn", "replace", "swap", "add", "change", "switch", "give", "get", "bring"];
        let start = t.words.iter().position(|x| verbs.contains(&x.as_str())).map(|i| i + 1).unwrap_or(0);
        let mut rest: Vec<String> = t.words[start..].to_vec();
        rest.retain(|w| w != "ai");
        while rest.first().map(|w| FILLER.contains(&w.as_str())).unwrap_or(false) {
            rest.remove(0);
        }
        // "spawn ai", "spawn 3 ai cars": no model named, so it's traffic
        let generic = ["car", "cars", "vehicle", "vehicles", "traffic", "driver", "drivers", "some", "few", "couple", "of"];
        if ai && rest.iter().all(|w| generic.contains(&w.as_str()) || w.parse::<u32>().is_ok() || ordinal(w).is_some()) {
            return Intent::Traffic(t.words.clone());
        }
        return Intent::Spawn { words: rest, add, ai };
    }
    if listy && t.any(&["cars", "vehicles", "car", "vehicle", "models"]) {
        return Intent::ListCars;
    }

    // --- drivetrain
    if t.has("diff") || (t.any(&["lock", "unlock"]) && t.any(&["front", "rear", "center", "axle", "axles", "inter"])) {
        if t.has("center") && (t.has("auto") || t.number().is_some()) {
            return Intent::CenterDiff(if t.has("auto") { None } else { t.number() });
        }
        let locked = !t.any(UNLOCK);
        let scope = if t.any(&["all", "both", "everything", "every"]) {
            "all"
        } else if t.has("inter") || t.phrase("inter axle") {
            "inter"
        } else {
            match (t.has("front"), t.has("rear"), t.has("center")) {
                (true, true, _) => "all",
                (true, false, false) => "front",
                (false, true, false) => "rear",
                (false, false, true) => "center",
                _ => "all",
            }
        };
        return Intent::DiffLock { scope, locked };
    }
    if t.has("range") || t.any(&["4lo", "4hi"]) || t.any_phrase(&["low ratio", "high ratio", "4 low", "four low", "4 high", "four high"]) {
        return Intent::Range(if t.any(&["low", "lo", "4lo"]) { "low" } else { "high" });
    }
    if t.any(&["4wd", "4x4", "awd", "2wd", "rwd", "fwd"])
        || t.any_phrase(&["four wheel drive", "4 wheel drive", "two wheel drive", "2 wheel drive", "all wheel drive"])
    {
        let two = t.any(&["2wd", "rwd", "fwd"]) || t.any_phrase(&["two wheel", "2 wheel"]) || off;
        return Intent::Drive(if two { "2wd" } else { "4wd" });
    }
    if t.any(&["traction", "tc", "tcs", "esc", "stability"]) {
        return Intent::Traction(!off);
    }

    // --- lights
    if t.any(&["hazard", "flashers"]) {
        return Intent::Hazard(!off);
    }
    if t.has("fog") {
        return Intent::Fog(!off);
    }
    if t.has("indicator") || t.phrase("turn signal") {
        let side = if off { "off" } else if t.has("left") { "left" } else if t.has("right") { "right" } else { "off" };
        return Intent::Signal(side);
    }
    if t.any(&["light", "beam", "headlight", "headlights", "brights", "highbeam", "highbeams", "lowbeam", "dipped"]) {
        let high = t.any(&["high", "full", "brights", "highbeam", "highbeams", "main"]);
        let low = t.any(&["low", "dipped", "lowbeam", "dip"]);
        return Intent::Lights(if high {
            if off { "low" } else { "high" }
        } else if low {
            "low"
        } else if off {
            "off"
        } else {
            "low"
        });
    }

    // --- gears
    if !t.any(&["brake", "parking", "handbrake"]) {
        if t.has("park") {
            return Intent::Gear("P".into());
        }
        if t.has("neutral") {
            return Intent::Gear("N".into());
        }
        if t.has("reverse") {
            return Intent::Gear("R".into());
        }
        if t.has("drive") && !t.any(&["wheel", "to"]) {
            return Intent::Gear("D".into());
        }
        if t.has("gear") {
            if t.any(&["sport", "s"]) {
                return Intent::Gear("S".into());
            }
            for w in &t.words {
                if let Some(n) = ordinal(w) {
                    return Intent::Gear(n.to_string());
                }
            }
        }
    }

    // --- readouts
    if t.any(&["temp", "temps", "temperature", "temperatures", "hot", "coolant"]) {
        return Intent::Read("temps");
    }
    if t.any(&["fuel", "gas", "petrol", "diesel", "tank"]) {
        return Intent::Read("fuel");
    }
    if t.any(&["rpm", "revs", "engine"]) {
        return Intent::Read("engine");
    }
    if t.any(&["speed", "fast"]) {
        return Intent::Read("speed");
    }
    if t.any(&["damage", "damaged", "broken", "condition"]) {
        return Intent::Damage;
    }
    if first == "load" {
        return Intent::LoadMap(strip_fillers(&t.words));
    }
    Intent::Unknown
}

/// Human label for what was requested (first half of the in-game message).
pub fn label(i: &Intent) -> String {
    let onoff = |b: &bool| if *b { "on" } else { "off" };
    match i {
        Intent::Lights("high") => "High beams".into(),
        Intent::Lights("low") => "Low beams".into(),
        Intent::Lights(_) => "Headlights off".into(),
        Intent::Fog(b) => format!("Fog lights {}", onoff(b)),
        Intent::Hazard(b) => format!("Hazards {}", onoff(b)),
        Intent::Signal("left") => "Left indicator".into(),
        Intent::Signal("right") => "Right indicator".into(),
        Intent::Signal(_) => "Indicators off".into(),
        Intent::DiffLock { scope, locked } => {
            let sc = match *scope {
                "all" => "All diffs",
                "inter" => "Inter-axle diffs",
                "front" => "Front diff",
                "rear" => "Rear diff",
                "center" => "Centre diff",
                o => o,
            };
            format!("{} {}", sc, if *locked { "lock" } else { "unlock" })
        }
        Intent::CenterDiff(Some(p)) => format!("Centre diff {}%", p),
        Intent::CenterDiff(None) => "Centre diff auto".into(),
        Intent::Range(r) => format!("{} range", if *r == "low" { "Low" } else { "High" }),
        Intent::Drive(d) => d.to_uppercase(),
        Intent::Traction(b) => format!("Traction control {}", onoff(b)),
        Intent::Gear(g) => format!("Gear {}", g),
        Intent::Read("temps") => "Temperatures".into(),
        Intent::Read("fuel") => "Fuel".into(),
        Intent::Read("engine") => "Engine".into(),
        Intent::Read(g) => g.to_string(),
        Intent::Damage => "Damage".into(),
        Intent::ListSwitches => "Switchable".into(),
        Intent::Repair => "Repair".into(),
        Intent::Respawn => "Respawn".into(),
        Intent::Spawn { words, add, ai } => {
            format!("{} {}", if *ai { "Spawn AI" } else if *add { "Add" } else { "Spawn" }, words.join(" "))
        }
        Intent::ListConfigs(_) => "Configs".into(),
        Intent::ListCars => "Vehicles".into(),
        Intent::ListMaps => "Maps".into(),
        Intent::CurrentMap => "Current map".into(),
        Intent::LoadMap(w) => format!("Load map {}", w.join(" ")),
        Intent::CloseMenu => "Close menu".into(),
        Intent::Paint(w) => format!("Paint {}", w.join(" ")),
        Intent::Env(_) => "Environment".into(),
        Intent::Help(_) => "Help".into(),
        Intent::Traffic(_) => "Traffic".into(),
        Intent::Ai(w) => format!("AI {}", w.join(" ")),
        Intent::Pause => "Pause".into(),
        Intent::Body { part, which, state } => {
            let what = match (*part, *which) {
                ("hood", _) => "Bonnet".to_string(),
                ("trunk", _) => "Boot".to_string(),
                (_, "all") | ("doors", "") => "All doors".to_string(),
                (_, "") => "Door".to_string(),
                (_, w) if w.len() == 2 => format!("{} door", w),
                (_, w) => format!("{} doors", w.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default() + &w[1..]),
            };
            format!("{} {}", what, state)
        }
        Intent::Engine(b) => (if *b { "Start engine" } else { "Stop engine" }).into(),
        Intent::Jato(b) => format!("JATO {}", onoff(b)),
        Intent::Nitrous(b) => (if *b { "Arm nitrous" } else { "Disarm nitrous" }).into(),
        Intent::EngineBrake { on, level, percent } => match (on, level, percent) {
            (false, _, _) => "Engine brake off".into(),
            (_, Some(l), _) => format!("Engine brake level {}", l),
            (_, _, Some(p)) => format!("Engine brake {}%", p),
            _ => "Engine brake on".into(),
        },
        Intent::Retarder(s) => {
            if s.parse::<u32>().is_ok() { format!("Retarder {}%", s) } else { format!("Retarder {}", s) }
        }
        Intent::ParkingBrake(b) => format!("Parking brake {}", onoff(b)),
        Intent::AirSuspension(s) => format!("Suspension {}", s),
        Intent::ListPlaces(_) => "Places".into(),
        Intent::Navigate(_) => "Satnav".into(),
        Intent::Distance(_) => "Distance".into(),
        Intent::ClearNav => "Clear satnav".into(),
        Intent::Autopilot(b) => format!("Autopilot {}", onoff(b)),
        Intent::DriveTo(_) => "Drive to".into(),
        Intent::Teleport(_) => "Teleport".into(),
        Intent::Unknown => "Unknown".into(),
    }
}

/// Word used in "already {state}".
pub fn state_word(i: &Intent) -> String {
    match i {
        Intent::Lights("off") => "off".into(),
        Intent::Lights(_) => "on".into(),
        Intent::Fog(b) | Intent::Hazard(b) | Intent::Traction(b) => (if *b { "on" } else { "off" }).to_string(),
        Intent::Signal("off") => "off".into(),
        Intent::Signal(_) => "on".into(),
        Intent::DiffLock { locked, .. } => (if *locked { "locked" } else { "open" }).to_string(),
        Intent::CenterDiff(Some(p)) => format!("at {}%", p),
        Intent::CenterDiff(None) => "auto".into(),
        Intent::Range(r) => format!("in {}", r),
        Intent::Drive(d) => format!("in {}", d.to_uppercase()),
        Intent::Gear(g) => format!("in {}", g),
        Intent::Body { state, .. } => (if *state == "close" { "closed" } else { "open" }).into(),
        Intent::Engine(b) => (if *b { "running" } else { "off" }).into(),
        Intent::Jato(b) | Intent::ParkingBrake(b) => (if *b { "on" } else { "off" }).into(),
        Intent::Nitrous(b) => (if *b { "armed" } else { "disarmed" }).into(),
        Intent::EngineBrake { on: false, .. } => "off".into(),
        Intent::EngineBrake { .. } => "at that level".into(),
        Intent::Retarder(s) => {
            if s.parse::<u32>().is_ok() { format!("at {}%", s) } else { s.clone() }
        }
        _ => "set".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases() {
        assert_eq!(parse("lock the rear diff"), Intent::DiffLock { scope: "rear", locked: true });
        assert_eq!(parse("unlock all differentials"), Intent::DiffLock { scope: "all", locked: false });
        assert_eq!(parse("set centre diff to 50%"), Intent::CenterDiff(Some(50)));
        assert_eq!(parse("center diff auto"), Intent::CenterDiff(None));
        assert_eq!(parse("low range"), Intent::Range("low"));
        assert_eq!(parse("four wheel drive on"), Intent::Drive("4wd"));
        assert_eq!(parse("4wd off"), Intent::Drive("2wd"));
        assert_eq!(parse("traction control off"), Intent::Traction(false));
        assert_eq!(parse("high beams"), Intent::Lights("high"));
        assert_eq!(parse("headlights off"), Intent::Lights("off"));
        assert_eq!(parse("fog lights on"), Intent::Fog(true));
        assert_eq!(parse("indicate left"), Intent::Signal("left"));
        assert_eq!(parse("cancel indicator"), Intent::Signal("off"));
        assert_eq!(parse("put it in park"), Intent::Gear("P".into()));
        assert_eq!(parse("what can i ask"), Intent::Help(None));
        assert_eq!(parse("what can you do"), Intent::Help(None));
        assert_eq!(parse("what commands are there"), Intent::Help(None));
        assert_eq!(parse("help"), Intent::Help(None));
        assert_eq!(parse("what vehicle commands are there"), Intent::Help(Some("vehicle")));
        assert_eq!(parse("what map commands can i use"), Intent::Help(Some("map")));
        assert_eq!(parse("what level commands are there"), Intent::Help(Some("map")));
        assert_eq!(parse("what game commands are there"), Intent::Help(Some("game")));
        assert_eq!(parse("help with weather"), Intent::Help(Some("env")));
        assert_eq!(parse("what can i ask about traffic"), Intent::Help(Some("ai")));
        assert_eq!(parse("what can i say to spawn cars"), Intent::Help(Some("spawn")));
        assert!(matches!(parse("what weather settings can i change"), Intent::Env(_)));
        assert_eq!(parse("third gear"), Intent::Gear("3".into()));
        assert_eq!(parse("repair the car"), Intent::Repair);
        assert_eq!(parse("restore"), Intent::Repair);
        assert_eq!(parse("respawn"), Intent::Respawn);
        assert_eq!(parse("reset the car"), Intent::Respawn);
        assert_eq!(parse("spawn bruckell legran"), Intent::Spawn { words: vec!["bruckell".into(), "legran".into()], add: false, ai: false });
        assert_eq!(parse("replace car with a pigeon"), Intent::Spawn { words: vec!["pigeon".into()], add: false, ai: false });
        assert_eq!(parse("list legran configs"), Intent::ListConfigs(vec!["legran".into()]));
        assert_eq!(parse("change map to west coast"), Intent::LoadMap(vec!["west".into(), "coast".into()]));
        assert_eq!(parse("what map is this"), Intent::CurrentMap);
        assert_eq!(parse("list maps"), Intent::ListMaps);
        assert_eq!(parse("what can i switch"), Intent::ListSwitches);
        assert_eq!(parse("engine temperature"), Intent::Read("temps"));
        assert_eq!(parse("close the menu"), Intent::CloseMenu);
        assert_eq!(parse("back to the game"), Intent::CloseMenu);
        assert_eq!(parse("unpause"), Intent::CloseMenu);
        assert!(matches!(parse("make the white sunburst black"), Intent::Paint(_)));
        assert!(matches!(parse("paint my car fire red"), Intent::Paint(_)));
        assert!(matches!(parse("change the colour of the pigeon to blue"), Intent::Paint(_)));
        assert_eq!(parse("make it high beam"), Intent::Lights("high"));
        let k = |s: &str| format!("{:?}", parse(s)).split('(').next().unwrap_or("").split(' ').next().unwrap_or("").to_string();
        for s in ["set the time to 6 pm", "make it sunset", "stop time", "make it foggy", "light rain", "clear skies",
                  "wind 20 km/h", "moon gravity", "slow motion", "what weather settings can i change", "what time is it",
                  "set time to half past 7"] {
            assert_eq!(k(s), "Env", "{}", s);
        }
        for s in ["spawn traffic", "remove traffic", "add 2 police cars", "spawn a police car", "more traffic"] {
            assert_eq!(k(s), "Traffic", "{}", s);
        }
        for s in ["make the white van chase me", "police chase me", "stop the red taxi", "make all traffic drive aggressively",
                  "tell the blue pickup to follow me", "make the cops run away"] {
            assert_eq!(k(s), "Ai", "{}", s);
        }
        assert_eq!(parse("autopilot on"), Intent::Autopilot(true));
        assert_eq!(parse("autopilot off"), Intent::Autopilot(false));
        assert!(matches!(parse("spawn ai"), Intent::Traffic(_)));
        assert!(matches!(parse("spawn 3 ai cars"), Intent::Traffic(_)));
        assert!(matches!(parse("spawn a police car"), Intent::Traffic(_)));
        assert_eq!(parse("pause the game"), Intent::Pause);
        assert_eq!(parse("pause"), Intent::Pause);
        assert_eq!(parse("open the doors"), Intent::Body { part: "doors", which: "all", state: "open" });
        assert_eq!(parse("open the driver door"), Intent::Body { part: "door", which: "FL", state: "open" });
        assert_eq!(parse("close the rear left door"), Intent::Body { part: "door", which: "RL", state: "close" });
        assert_eq!(parse("open the bonnet"), Intent::Body { part: "hood", which: "", state: "open" });
        assert_eq!(parse("shut the boot"), Intent::Body { part: "trunk", which: "", state: "close" });
        assert_eq!(parse("start the engine"), Intent::Engine(true));
        assert_eq!(parse("start the car"), Intent::Engine(true));
        assert_eq!(parse("fire it up"), Intent::Engine(true));
        assert_eq!(parse("kill the engine"), Intent::Engine(false));
        assert_eq!(parse("turn the engine off"), Intent::Engine(false));
        assert_eq!(parse("jato on"), Intent::Jato(true));
        assert_eq!(parse("fire the rockets"), Intent::Jato(true));
        assert_eq!(parse("jato off"), Intent::Jato(false));
        assert_eq!(parse("arm the nitrous"), Intent::Nitrous(true));
        assert_eq!(parse("disarm nos"), Intent::Nitrous(false));
        assert_eq!(parse("engine brake level 2"), Intent::EngineBrake { on: true, level: Some(2), percent: None });
        assert_eq!(parse("jake brake off"), Intent::EngineBrake { on: false, level: None, percent: None });
        assert_eq!(parse("engine brake on"), Intent::EngineBrake { on: true, level: None, percent: None });
        assert_eq!(parse("retarder 60%"), Intent::Retarder("60".into()));
        assert_eq!(parse("retarder auto"), Intent::Retarder("auto".into()));
        assert_eq!(parse("retarder off"), Intent::Retarder("off".into()));
        assert_eq!(parse("release the handbrake"), Intent::ParkingBrake(false));
        assert_eq!(parse("parking brake on"), Intent::ParkingBrake(true));
        assert_eq!(parse("raise the suspension"), Intent::AirSuspension("raise"));
        assert_eq!(parse("lower the suspension"), Intent::AirSuspension("lower"));
        assert!(matches!(parse("navigate to chinatown"), Intent::Navigate(_)));
        assert!(matches!(parse("set satnav to the drag strip"), Intent::Navigate(_)));
        assert!(matches!(parse("set the satnav for the quarry"), Intent::Navigate(_)));
        assert!(matches!(parse("show me how to get to the race track"), Intent::Navigate(_)));
        assert!(matches!(parse("where is the nearest gas station"), Intent::Navigate(_)));
        assert!(matches!(parse("find the nearest charging station"), Intent::Navigate(_)));
        assert!(matches!(parse("how far is the nearest service station"), Intent::Distance(_)));
        assert!(matches!(parse("what is the distance to the dockyards"), Intent::Distance(_)));
        assert!(matches!(parse("what direction is the quarry"), Intent::Distance(_)));
        assert_eq!(parse("cancel satnav"), Intent::ClearNav);
        assert_eq!(parse("clear the route"), Intent::ClearNav);
        assert!(matches!(parse("take me to the dockyards"), Intent::DriveTo(_)));
        assert!(matches!(parse("drive me to the nearest gas station"), Intent::DriveTo(_)));
        assert!(matches!(parse("teleport to the highway"), Intent::Teleport(_)));
        assert!(matches!(parse("spawn at the race track"), Intent::Teleport(_)));
        assert!(matches!(parse("list places"), Intent::ListPlaces(_)));
        assert!(matches!(parse("list gas stations"), Intent::ListPlaces(_)));
        assert!(matches!(parse("where can i go"), Intent::ListPlaces(_)));
        assert!(matches!(parse("wind 20 km/h"), Intent::Env(_)));
        assert!(matches!(parse("set wind direction to north"), Intent::Env(_)));
        assert_eq!(parse("drive"), Intent::Gear("D".into()));
        assert_eq!(parse("engine temperature"), Intent::Read("temps"));
        assert!(matches!(parse("change map to west coast"), Intent::LoadMap(_)));
        assert_eq!(parse("spawn an ai van"), Intent::Spawn { words: vec!["van".into()], add: true, ai: true });
        assert_eq!(parse("fog lights on"), Intent::Fog(true));
        assert_eq!(parse("headlights on"), Intent::Lights("low"));
        assert_eq!(parse("put it in park"), Intent::Gear("P".into()));
        assert_eq!(parse("what can i ask"), Intent::Help(None));
        assert_eq!(parse("what can you do"), Intent::Help(None));
        assert_eq!(parse("what commands are there"), Intent::Help(None));
        assert_eq!(parse("help"), Intent::Help(None));
        assert_eq!(parse("what vehicle commands are there"), Intent::Help(Some("vehicle")));
        assert_eq!(parse("what map commands can i use"), Intent::Help(Some("map")));
        assert_eq!(parse("what level commands are there"), Intent::Help(Some("map")));
        assert_eq!(parse("what game commands are there"), Intent::Help(Some("game")));
        assert_eq!(parse("help with weather"), Intent::Help(Some("env")));
        assert_eq!(parse("what can i ask about traffic"), Intent::Help(Some("ai")));
        assert_eq!(parse("what can i say to spawn cars"), Intent::Help(Some("spawn")));
        assert!(matches!(parse("what weather settings can i change"), Intent::Env(_)));
    }
}
