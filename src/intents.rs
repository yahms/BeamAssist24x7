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
        "game" | "menu" | "ui" => "game",
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

    // --- paint
    let last_is_colour = t.words.last().map(|w| COLOURS.contains(&w.as_str())).unwrap_or(false);
    if t.any(&["paint", "respray", "repaint", "recolour", "recolor", "colour", "color"]) || (first == "make" && last_is_colour) {
        return Intent::Paint(t.words.clone());
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
                  "autopilot on", "tell the blue pickup to follow me", "make the cops run away"] {
            assert_eq!(k(s), "Ai", "{}", s);
        }
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
