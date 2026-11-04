//! Time of day, weather and other environment settings (core_environment in the GE VM).
//! Notes from live testing:
//!  - native time of day: 0 = noon, 0.5 = midnight (the MCP set_time_of_day doc says otherwise)
//!  - named times (sunrise, evening...) are per level: core_environment.getTimeOfDayOptions()
//!  - core_weather presets fail in this game build (object fields missing), so weather is
//!    driven directly: cloud cover, fog density, wind speed, rain (Precipitation.numDrops)
//!  - temperature follows the level's temperature profile (read-only here)

use anyhow::Result;
use serde_json::{json, Value};

use crate::lua::arr;
use crate::mcp::Mcp;
use crate::paint::Outcome;

/// Fog density at "100%". The level default on gridmap is 0.0003.
const FOG_FULL: f64 = 0.01;

const GRAVITY: &[(&str, f64)] = &[
    ("earth", -9.81),
    ("normal", -9.81),
    ("moon", -1.62),
    ("mars", -3.71),
    ("sun", -274.0),
    ("jupiter", -24.92),
    ("neptune", -11.15),
    ("saturn", -10.44),
    ("uranus", -8.87),
    ("venus", -8.87),
    ("mercury", -3.7),
    ("pluto", -0.58),
    ("zero", 0.0),
    ("none", 0.0),
    ("negative", 9.81),
    ("reverse", 9.81),
];

const TIME_WORDS: &[&str] = &[
    "time", "clock", "oclock", "am", "pm", "noon", "midday", "midnight", "sunrise", "dawn", "morning", "afternoon",
    "evening", "sunset", "dusk", "night", "day", "daytime", "nighttime",
];
const WEATHER_WORDS: &[&str] = &[
    "weather", "clear", "sunny", "cloud", "clouds", "cloudy", "overcast", "fog", "foggy", "mist", "misty", "wind",
    "windy", "breeze", "calm", "rain", "raining", "rainy", "storm", "stormy", "drizzle",
];

/// Does this sentence look like an environment command? (Used by the intent parser.)
pub fn is_env(words: &[String]) -> bool {
    let has = |w: &str| words.iter().any(|x| x == w);
    let any = |ws: &[&str]| ws.iter().any(|w| has(*w));
    if has("fog") && any(&["light", "lamp", "lamps"]) {
        return false; // fog lights
    }
    any(TIME_WORDS)
        || any(WEATHER_WORDS)
        || has("gravity")
        || any(&["environment", "temperature"])
        || (has("slow") && has("motion"))
        || (any(&["sim", "simulation", "game"]) && has("speed"))
        || (any(&["half", "quarter", "normal", "full", "real"]) && any(&["speed", "time"]) && !any(&["traffic", "car"]))
}

fn num(w: &str) -> Option<f64> {
    w.trim_end_matches('%').parse::<f64>().ok()
}

/// Percentage or level word near a keyword.
fn amount(words: &[String], levels: &[(&str, f64)]) -> Option<f64> {
    for w in words {
        if let Some(n) = num(w) {
            return Some((n / 100.0).clamp(0.0, 1.0));
        }
    }
    for w in words {
        if let Some((_, v)) = levels.iter().find(|(k, _)| k == w) {
            return Some(*v);
        }
    }
    None
}

fn state(mcp: &Mcp) -> Result<Value> {
    let code = "local s=core_environment.getState() \
        local rain=nil local ok,r=pcall(core_environment.getPrecipitation) if ok then rain=r end \
        local opts={} local ok2,o=pcall(core_environment.getTimeOfDayOptions) if ok2 and o then for _,x in ipairs(o) do opts[#opts+1]={key=x.key,value=x.value} end end \
        return jsonEncode({time=s.time,play=s.play,dayLength=s.dayLength,cloud=s.cloudCover,fog=core_environment.getFogDensity(), \
        wind=s.windSpeed,tempC=s.temperatureC,gravity=s.gravity,rain=rain,hasRain=(rain~=nil),times=opts})";
    Ok(serde_json::from_str(&mcp.run_lua(code)?)?)
}

fn clock(t: f64) -> String {
    let h = (t * 24.0 + 12.0).rem_euclid(24.0);
    let (hh, mm) = (h.floor() as u32, ((h - h.floor()) * 60.0).round() as u32);
    let (hh, mm) = if mm == 60 { ((hh + 1) % 24, 0) } else { (hh, mm) };
    format!("{:02}:{:02}", hh, mm)
}

fn gravity_name(g: f64) -> String {
    GRAVITY
        .iter()
        .find(|(_, v)| (v - g).abs() < 0.05)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| format!("{:.2} m/s²", -g))
}

fn sim_speed(mcp: &Mcp) -> f64 {
    mcp.call_json("get_simulation_state", json!({}))
        .ok()
        .and_then(|s| s["timeScale"].as_f64())
        .unwrap_or(1.0)
}

/// "What can I change?" — every variable with its current value.
pub fn list(mcp: &Mcp) -> Result<Outcome> {
    let s = state(mcp)?;
    let t = s["time"].as_f64().unwrap_or(0.0);
    let times: Vec<String> = arr(&s["times"]).iter().filter_map(|x| x["key"].as_str().map(String::from)).collect();
    let rain = if s["hasRain"].as_bool().unwrap_or(false) {
        format!("{:.0} drops", s["rain"].as_f64().unwrap_or(0.0))
    } else {
        "not on this map".into()
    };
    let mut lines = vec![
        format!(
            "time {} ({}), day length {:.0} min",
            clock(t),
            if s["play"].as_bool().unwrap_or(false) { "running" } else { "stopped" },
            s["dayLength"].as_f64().unwrap_or(0.0) / 60.0
        ),
        format!("clouds {:.0}%", s["cloud"].as_f64().unwrap_or(0.0) * 100.0),
        format!("fog {:.0}%", s["fog"].as_f64().unwrap_or(0.0) / FOG_FULL * 100.0),
        format!("wind {:.1} m/s", s["wind"].as_f64().unwrap_or(0.0)),
        format!("rain {}", rain),
        format!("gravity {}", gravity_name(s["gravity"].as_f64().unwrap_or(-9.81))),
        format!("sim speed {}×", sim_speed(mcp)),
        format!("temperature {:.0}°C (read-only)", s["tempC"].as_f64().unwrap_or(0.0)),
    ];
    if !times.is_empty() {
        lines.push(format!("named times: {}", times.join(", ")));
    }
    for l in &lines {
        println!("  {}", l);
    }
    Ok(Outcome { label: "Environment".into(), text: lines.join("; "), status: "applied" })
}

fn lua(mcp: &Mcp, code: &str) -> Result<()> {
    mcp.run_lua(&format!("{} return 'ok'", code))?;
    Ok(())
}

pub fn run(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let has = |w: &str| words.iter().any(|x| x == w);
    let any = |ws: &[&str]| ws.iter().any(|w| has(*w));
    let first = words.first().map(|s| s.as_str()).unwrap_or("");
    if ["what", "whats", "which", "list", "show", "tell", "how"].contains(&first)
        || any(&["options", "settings", "variables", "available"])
    {
        return list(mcp);
    }

    let before = state(mcp)?;
    let mut done: Vec<String> = vec![];
    let mut problems: Vec<String> = vec![];

    // ---------------- time
    if any(TIME_WORDS) {
        let off = any(&["stop", "freeze", "pause", "halt"]);
        let on = any(&["start", "resume", "unfreeze", "run", "running", "pass", "flow", "unpause"]);
        if has("length") || (has("day") && any(&["minutes", "minute", "hours", "hour", "seconds"])) {
            if let Some(n) = words.iter().find_map(|w| num(w)) {
                let secs = if any(&["hours", "hour"]) { n * 3600.0 } else if any(&["seconds"]) { n } else { n * 60.0 };
                lua(mcp, &format!("core_environment.setTimeOfDay({{dayLength={}}})", secs))?;
                done.push(format!("day length {:.0} min", secs / 60.0));
            }
        } else if (off || on) && any(&["time", "clock", "day"]) && !any(&["am", "pm", "oclock"]) {
            lua(mcp, &format!("core_environment.setTimeOfDay({{play={}}})", on && !off))?;
            done.push(if on && !off { "time running".into() } else { "time stopped".into() });
        } else if let Some(t) = parse_time(words, &before) {
            lua(mcp, &format!("core_environment.setTimeOfDay({{time={}}})", t))?;
            done.push(format!("time {}", clock(t)));
        }
    }

    // ---------------- weather
    let clear = any(&["clear", "sunny"]) && !any(&["cloud", "clouds", "cloudy"]);
    let stormy = any(&["storm", "stormy"]);
    if clear {
        lua(mcp, "core_environment.setCloudCover(0) core_environment.setFogDensity(0) \
                  for _,n in ipairs(scenetree.findClassObjects('Precipitation')) do local o=scenetree.findObject(n) if o then o.numDrops=0 end end")?;
        done.push("clear skies".into());
    }
    if any(&["cloud", "clouds", "cloudy", "overcast"]) || stormy {
        let off = any(&["no", "off", "remove", "none"]);
        let v = if off {
            0.0
        } else if stormy || has("overcast") {
            1.0
        } else {
            amount(words, &[("few", 0.25), ("light", 0.3), ("partly", 0.4), ("some", 0.4), ("heavy", 0.9), ("full", 1.0)])
                .unwrap_or(0.75)
        };
        lua(mcp, &format!("core_environment.setCloudCover({})", v))?;
        done.push(format!("clouds {:.0}%", v * 100.0));
    }
    if any(&["fog", "foggy", "mist", "misty"]) {
        let off = any(&["no", "off", "remove", "none", "clear"]);
        let frac = if off {
            0.0
        } else {
            amount(words, &[("light", 0.08), ("thin", 0.08), ("some", 0.2), ("medium", 0.3), ("heavy", 0.8), ("thick", 0.8), ("dense", 1.0)])
                .unwrap_or(0.3)
        };
        lua(mcp, &format!("core_environment.setFogDensity({})", frac * FOG_FULL))?;
        done.push(format!("fog {:.0}%", frac * 100.0));
    }
    if any(&["wind", "windy", "breeze", "calm"]) || stormy {
        let off = any(&["no", "off", "stop", "none"]) || has("calm");
        let mut ms = if off { 0.0 } else if stormy { 20.0 } else { 8.0 };
        if !off {
            if let Some(i) = words.iter().position(|w| num(w).is_some()) {
                let n = num(&words[i]).unwrap_or(0.0);
                let unit = words.get(i + 1).map(|s| s.as_str()).unwrap_or("");
                ms = match unit {
                    "kmh" | "km" | "kph" | "kilometres" | "kilometers" => n / 3.6,
                    "mph" | "miles" => n * 0.447,
                    "knots" | "knot" => n * 0.514,
                    _ => n,
                };
            } else if any(&["light", "gentle", "breeze"]) {
                ms = 3.0;
            } else if any(&["strong", "high", "gale", "heavy"]) {
                ms = 15.0;
            }
        }
        lua(mcp, &format!("core_environment.setWindSpeed({})", ms))?;
        done.push(format!("wind {:.1} m/s", ms));
    }
    if any(&["rain", "raining", "rainy", "drizzle"]) || stormy {
        if !before["hasRain"].as_bool().unwrap_or(false) {
            problems.push("this map has no rain".into());
        } else {
            let off = any(&["no", "off", "stop", "none"]);
            let drops = if off {
                0.0
            } else if stormy || any(&["heavy", "pouring", "hard"]) {
                3000.0
            } else if any(&["light", "drizzle"]) || has("drizzle") {
                400.0
            } else {
                amount(words, &[]).map(|f| f * 3000.0).unwrap_or(1200.0)
            };
            // Rain maps have several Precipitation objects (e.g. Small Island: "rain_single_drops"
            // with the rain_drop datablock, and "rain_coverage" sheet rain); set them all.
            lua(
                mcp,
                &format!(
                    "for _,n in ipairs(scenetree.findClassObjects('Precipitation')) do local o=scenetree.findObject(n) \
                     if o then if o:getField('dataBlock','')=='rain_drop' then o.numDrops={d} else o.numDrops={c} end end end",
                    d = drops,
                    c = (drops / 2.0).round()
                ),
            )?;
            if drops > 0.0 && !any(&["cloud", "clouds", "cloudy", "clear", "sunny"]) {
                lua(mcp, "core_environment.setCloudCover(math.max(core_environment.getCloudCover() or 0, 0.85))")?;
            }
            done.push(if drops == 0.0 { "rain off".into() } else { format!("rain {:.0} drops", drops) });
        }
    }

    // ---------------- gravity
    if has("gravity") || (has("moon") && !any(TIME_WORDS)) {
        let g = GRAVITY
            .iter()
            .find(|(n, _)| has(*n))
            .map(|(_, v)| *v)
            .or_else(|| words.iter().find_map(|w| num(w)).map(|n| -n.abs()));
        match g {
            Some(g) => {
                lua(mcp, &format!("core_environment.setGravity({})", g))?;
                done.push(format!("gravity {}", gravity_name(g)));
            }
            None => problems.push("which gravity? earth, moon, mars, jupiter, zero…".into()),
        }
    }

    // ---------------- simulation speed
    if (has("slow") && has("motion")) || (any(&["sim", "simulation", "game", "half", "quarter", "normal", "full", "real"]) && any(&["speed", "time"]) && !has("past")) {
        let off = any(&["off", "stop", "normal", "real", "full"]);
        let scale = if off {
            1.0
        } else if has("quarter") {
            0.25
        } else if has("half") {
            0.5
        } else if let Some(n) = words.iter().find_map(|w| num(w)) {
            if n > 1.0 { (n / 100.0).clamp(0.01, 1.0) } else { n }
        } else {
            0.25
        };
        mcp.call("set_simulation_speed", json!({ "scale": scale }))?;
        done.push(if scale == 1.0 { "real-time speed".into() } else { format!("sim speed {}×", scale) });
    }

    if has("temperature") && done.is_empty() {
        problems.push(format!(
            "temperature follows the map's profile ({:.0}°C now)",
            before["tempC"].as_f64().unwrap_or(0.0)
        ));
    }

    let label = "Environment".to_string();
    if done.is_empty() {
        let text = if problems.is_empty() { "not understood; say 'what environment settings can I change'".into() } else { problems.join("; ") };
        return Ok(Outcome { label, text, status: if problems.is_empty() { "failed" } else { "unsupported" } });
    }
    let mut text = done.join(", ");
    if !problems.is_empty() {
        text = format!("{}; {}", text, problems.join("; "));
    }
    Ok(Outcome { label, text, status: if problems.is_empty() { "applied" } else { "partial" } })
}

/// Clock times ("6 pm", "18 30", "half past 7"), noon/midnight, or level-named times.
fn parse_time(words: &[String], st: &Value) -> Option<f64> {
    let has = |w: &str| words.iter().any(|x| x == w);
    let to_native = |h: f64| ((h - 12.0) / 24.0).rem_euclid(1.0);
    if has("noon") || has("midday") {
        return Some(to_native(12.0));
    }
    if has("midnight") {
        return Some(to_native(0.0));
    }
    // numeric clock
    for (i, w) in words.iter().enumerate() {
        let Ok(h) = w.parse::<u32>() else { continue };
        if h > 24 {
            continue;
        }
        let mut hour = h as f64;
        let mut minute = 0.0;
        if let Some(m) = words.get(i + 1).and_then(|m| m.parse::<u32>().ok()) {
            if m < 60 {
                minute = m as f64;
            }
        }
        let pm = has("pm") || has("evening") || (has("night") && (6..12).contains(&h));
        let am = has("am") || has("morning");
        if pm && hour < 12.0 {
            hour += 12.0;
        }
        if am && hour == 12.0 {
            hour = 0.0;
        }
        if has("half") && has("past") {
            minute = 30.0;
        }
        return Some(to_native(hour + minute / 60.0));
    }
    // level-specific named times
    let named = [
        ("sunrise", "sunrise"),
        ("dawn", "sunrise"),
        ("morning", "morning"),
        ("afternoon", "afternoon"),
        ("evening", "evening"),
        ("sunset", "sunset"),
        ("dusk", "sunset"),
        ("night", "night"),
        ("nighttime", "night"),
    ];
    for (w, key) in named {
        if has(w) {
            if let Some(v) = arr(&st["times"]).iter().find(|x| x["key"] == key).and_then(|x| x["value"].as_f64()) {
                return Some(v);
            }
        }
    }
    if has("day") || has("daytime") {
        return Some(to_native(12.0));
    }
    None
}
