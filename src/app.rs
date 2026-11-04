//! Executes intents: resolver calls for vehicle functions, MCP tools for game-level
//! actions, and composes the in-game message "{intent} — {result}".

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::catalog::{match_config, Catalog};
use crate::config::Config;
use crate::intents::{self, Intent};
use crate::lua::{self, arr};
use crate::mcp::Mcp;

pub struct App {
    pub mcp: Mcp,
    pub cfg: Config,
    pub cat: Catalog,
    /// model key -> config key last used this session (in memory only)
    last_config: HashMap<String, String>,
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        o => o.to_string(),
    }
}

fn name_of(t: &Value) -> String {
    let ui = s(&t["ui"]);
    if ui.is_empty() { s(&t["device"]) } else { ui }
}

/// Map an intent to a resolver action + args, if it is a vehicle-VM function.
fn resolver_call(i: &Intent) -> Option<(&'static str, Value)> {
    let onoff = |b: &bool| if *b { "on" } else { "off" };
    Some(match i {
        Intent::Lights(st) => ("lights", json!({ "state": st })),
        Intent::Fog(b) => ("fog", json!({ "state": onoff(b) })),
        Intent::Hazard(b) => ("hazard", json!({ "state": onoff(b) })),
        Intent::Signal(side) => ("signal", json!({ "side": side })),
        Intent::DiffLock { scope, locked } => {
            ("diff_lock", json!({ "scope": scope, "state": if *locked { "locked" } else { "open" } }))
        }
        Intent::CenterDiff(Some(p)) => ("center_diff", json!({ "percent": p })),
        Intent::CenterDiff(None) => ("center_diff", json!({ "mode": "auto" })),
        Intent::Range(r) => ("range", json!({ "state": r })),
        Intent::Drive(d) => ("drive", json!({ "state": d })),
        Intent::Traction(b) => ("traction", json!({ "state": onoff(b) })),
        Intent::Gear(g) => ("gear", json!({ "state": g })),
        Intent::Read(g) => ("read", json!({ "group": g })),
        Intent::ListSwitches => ("list", json!({})),
        _ => return None,
    })
}

impl App {
    pub fn new(mcp: Mcp, cfg: Config, cat: Catalog) -> Self {
        App { mcp, cfg, cat, last_config: HashMap::new() }
    }

    fn tmpl(&self, key: &str, vars: &[(&str, String)]) -> String {
        let mut t = self.cfg.messages.result.get(key).cloned().unwrap_or_else(|| key.to_string());
        for (k, v) in vars {
            t = t.replace(&format!("{{{}}}", k), v);
        }
        t.trim().to_string()
    }

    /// Print and show "{intent} — {result}" in game.
    pub fn say(&self, label: &str, result: &str, status: &str, ttl: Option<f64>) {
        let m = &self.cfg.messages;
        let text = m.format.replace("{intent}", label).replace("{result}", result);
        let icon = m.icons.get(status).cloned().unwrap_or_else(|| "info".into());
        let ttl = ttl.unwrap_or(if matches!(status, "failed" | "unsupported" | "partial") { m.ttl_error } else { m.ttl });
        println!("  [{}] {}", status, text);
        if let Err(e) = self.mcp.show_message(&text, &icon, ttl, &m.category) {
            eprintln!("  (show_message failed: {:#})", e);
        }
    }

    pub fn handle(&mut self, line: &str) {
        let intent = intents::parse(line);
        println!("  intent: {:?}", intent);
        let label = intents::label(&intent);
        if let Err(e) = self.dispatch(line, &intent) {
            let msg = format!("{:#}", e);
            let txt = if msg.contains("no response from vehicle") {
                self.tmpl("no_response", &[])
            } else {
                self.tmpl("failed", &[("detail", msg)])
            };
            self.say(&label, &txt, "failed", None);
        }
    }

    fn dispatch(&mut self, line: &str, intent: &Intent) -> Result<()> {
        if let Some((action, args)) = resolver_call(intent) {
            let r = lua::resolve(&self.mcp, action, &args)?;
            println!("  {}", serde_json::to_string(&r).unwrap_or_default());
            self.report(intent, &r);
            return Ok(());
        }
        let label = intents::label(intent);
        match intent {
            Intent::Repair => {
                self.mcp.call("recover_vehicle", json!({}))?;
                self.say(&label, &self.tmpl("applied", &[]), "applied", None);
            }
            Intent::Respawn => {
                self.mcp.call("trigger_action", json!({ "action": "loadHome" }))?;
                self.say(&label, &self.tmpl("applied", &[]), "applied", None);
            }
            Intent::Damage => {
                let d = self.mcp.call_json("get_vehicle_damage", json!({}))?;
                let sum = d["damageSum"].as_f64().unwrap_or(0.0);
                let level = if sum < 10.0 {
                    "no damage"
                } else if sum < 20_000.0 {
                    "light damage"
                } else if sum < 500_000.0 {
                    "moderate damage"
                } else {
                    "heavy damage"
                };
                let txt = if sum < 10.0 { level.to_string() } else { format!("{} (score {:.0})", level, sum) };
                self.say(&label, &txt, "applied", None);
            }
            Intent::Spawn { words, add, ai } => self.spawn(words, *add, *ai)?,
            Intent::Help(cat) => {
                let (label, text, console) = crate::help::guide(*cat);
                println!("{}", console);
                self.say(&label, &text, "applied", Some(self.cfg.messages.ttl_list));
            }
            Intent::Env(words) => {
                let o = crate::env::run(&self.mcp, words)?;
                let ttl = if o.label == "Environment" && o.text.contains("named times") { Some(self.cfg.messages.ttl_list) } else { None };
                self.say(&o.label, &o.text, o.status, ttl);
            }
            Intent::Traffic(words) => {
                let o = crate::ai::traffic(&self.mcp, words)?;
                self.say(&o.label, &o.text, o.status, None);
            }
            Intent::Ai(words) => {
                let o = crate::ai::command(&self.mcp, words)?;
                self.say(&o.label, &o.text, o.status, None);
            }
            Intent::ListConfigs(words) => self.list_configs(words)?,
            Intent::ListCars => {
                let mut by_brand: Vec<(String, Vec<String>)> = vec![];
                for m in &self.cat.models {
                    if m.kind == "Prop" || m.kind == "Utility" {
                        continue;
                    }
                    match by_brand.iter_mut().find(|(b, _)| *b == m.brand) {
                        Some((_, v)) => v.push(m.name.clone()),
                        None => by_brand.push((m.brand.clone(), vec![m.name.clone()])),
                    }
                }
                for (b, v) in &by_brand {
                    println!("  {}: {}", if b.is_empty() { "(no brand)" } else { b.as_str() }, v.join(", "));
                }
                let n: usize = by_brand.iter().map(|(_, v)| v.len()).sum();
                let brands: Vec<&str> = by_brand.iter().map(|(b, _)| b.as_str()).filter(|b| !b.is_empty()).collect();
                let txt = format!("{} vehicles: {}", n, brands.join(", "));
                self.say(&label, &txt, "applied", Some(self.cfg.messages.ttl_list));
            }
            Intent::ListMaps => {
                let titles: Vec<&str> = self.cat.levels.iter().map(|l| l.title.as_str()).collect();
                self.say(&label, &titles.join(", "), "applied", Some(self.cfg.messages.ttl_list));
            }
            Intent::CurrentMap => {
                let name = self.current_level()?;
                let title = self
                    .cat
                    .levels
                    .iter()
                    .find(|l| l.name.eq_ignore_ascii_case(&name))
                    .map(|l| l.title.clone())
                    .unwrap_or(name);
                self.say(&label, &title, "applied", None);
            }
            Intent::LoadMap(words) => self.load_map(words)?,
            Intent::Paint(words) => {
                let o = crate::paint::run(&self.mcp, &self.cat, words)?;
                self.say(&o.label, &o.text, o.status, None);
            }
            Intent::CloseMenu => {
                let closed = self.ensure_play()?;
                let txt = if closed { self.tmpl("applied", &[]) } else { self.tmpl("already", &[("state", "in game".into())]) };
                self.say(&label, &txt, if closed { "applied" } else { "already" }, None);
            }
            Intent::Unknown => {
                if self.cfg.messages.show_not_understood {
                    self.say(&format!("Heard '{}'", line), &self.tmpl("not_understood", &[]), "failed", None);
                } else {
                    println!("  not understood");
                }
            }
            _ => return Err(anyhow!("not implemented")),
        }
        Ok(())
    }

    /// Turn a resolver result into the in-game message.
    fn report(&self, intent: &Intent, r: &Value) {
        let label = intents::label(intent);
        let status = s(&r["status"]);
        match intent {
            Intent::Read(g) => return self.say(&label, &fmt_read(g, &r["data"]), "applied", None),
            Intent::ListSwitches => {
                println!("{}", serde_json::to_string_pretty(&r["data"]).unwrap_or_default());
                return self.say(&label, &fmt_switches(&r["data"]), "applied", Some(self.cfg.messages.ttl_list));
            }
            _ => {}
        }
        let targets = arr(&r["targets"]);
        let total = targets.len();
        let ok = targets.iter().filter(|t| t["ok"].as_bool().unwrap_or(false)).count();
        let text = match status.as_str() {
            "applied" => {
                let mut t = self.tmpl("applied", &[]);
                if total > 1 {
                    t = format!("{} ({})", t, total);
                }
                let side: Vec<String> = arr(&r["side"]).iter().map(|x| format!("{} {}", name_of(x), s(&x["to"]))).collect();
                if !side.is_empty() {
                    t = format!("{}; also {}", t, side.join(", "));
                }
                t
            }
            "already" => self.tmpl("already", &[("state", intents::state_word(intent))]),
            "partial" => {
                let missed: Vec<String> = targets
                    .iter()
                    .filter(|t| !t["ok"].as_bool().unwrap_or(false))
                    .map(|t| name_of(t))
                    .collect();
                format!(
                    "{}; not changed: {}",
                    self.tmpl("partial", &[("ok", ok.to_string()), ("total", total.to_string())]),
                    missed.join(", ")
                )
            }
            "unsupported" => {
                let mut t = self.tmpl("unsupported", &[]);
                if let Some(d) = r["detail"].as_str() {
                    t = format!("{} ({})", t, d);
                }
                t
            }
            _ => self.tmpl("failed", &[("detail", r["detail"].as_str().unwrap_or("").to_string())]),
        };
        self.say(&label, &text, &status, None);
    }

    /// Close any menu/pause screen over the game: navigate to "play" until the UI
    /// route reports "play" (closing the menu sometimes lands on "pause" first), then
    /// make sure physics is running. Returns true if anything had to be closed.
    pub fn ensure_play(&self) -> Result<bool> {
        let route = |app: &App| -> Result<String> {
            let u = app.mcp.call_json("get_ui_state", json!({}))?;
            Ok(s(&u["route"]["currentRoute"]["resolved"]["name"]))
        };
        // Only when a map with a vehicle is loaded; otherwise the main menu is where the player is.
        let st = self.status()?;
        if s(&st["level"]).is_empty() || !st["vehicle"].is_object() {
            return Ok(false);
        }
        // Maps loaded through MCP don't load the quick-access (radial menu) extension, so
        // the pause menu names vehicles "Vehicle <id>" instead of their model/config.
        let _ = self.mcp.run_lua("if not core_quickAccess then extensions.load('core_quickAccess') end return 'ok'");
        let mut closed = false;
        // The pause screen also freezes simulation time (timeScale 0) while physicsRunning
        // stays true, so check the game's pause flag directly.
        if self.mcp.run_lua("return tostring(simTimeAuthority.getPause())")?.trim() == "true" {
            println!("  game paused — unpausing");
            self.mcp.run_lua("simTimeAuthority.pause(false) return 'ok'")?;
            closed = true;
            std::thread::sleep(Duration::from_millis(500));
        }
        for _ in 0..5 {
            let r = route(self)?;
            if r == "play" {
                break;
            }
            println!("  UI on '{}' — returning to game", r);
            self.mcp.call("set_ui_state", json!({ "route": "play" }))?;
            closed = true;
            std::thread::sleep(Duration::from_millis(800));
        }
        if let Ok(sim) = self.mcp.call_json("get_simulation_state", json!({})) {
            if sim["physicsRunning"].as_bool() == Some(false) {
                println!("  physics paused — resuming");
                self.mcp.call("resume_physics", json!({}))?;
                closed = true;
            }
        }
        Ok(closed)
    }

    fn status(&self) -> Result<Value> {
        self.mcp.call_json("get_status", json!({}))
    }

    fn current_level(&self) -> Result<String> {
        let st = self.status()?;
        let path = s(&st["level"]);
        // "/levels/<name>/main.level.json"
        Ok(path.split('/').nth(2).unwrap_or(&path).to_string())
    }

    fn spawn(&mut self, words: &[String], add: bool, ai: bool) -> Result<()> {
        let spoken = words.join(" ");
        let (mi, used, _) = match self.cat.match_model(words) {
            Some(m) => m,
            None => {
                self.say(&format!("Spawn '{}'", spoken), "vehicle not found", "failed", None);
                return Ok(());
            }
        };
        let model = self.cat.models[mi].clone();
        let label = format!("{} {}", if ai { "Spawn AI" } else if add { "Add" } else { "Spawn" }, model.label());
        let rest: Vec<String> = words[used..]
            .iter()
            .filter(|w| !["config", "configuration", "in", "with", "the", "a", "spec"].contains(&w.as_str()))
            .cloned()
            .collect();

        // Remember what the player is driving now (covers vehicles spawned from the game UI too).
        if let Ok(st) = self.status() {
            let (j, c) = (s(&st["vehicle"]["jbeam"]), s(&st["vehicle"]["configKey"]));
            if !j.is_empty() && !c.is_empty() {
                self.last_config.insert(j, c);
            }
        }

        let (default, cfgs) = self.cat.configs(&self.mcp, &model.key)?;
        let chosen = if rest.is_empty() {
            None
        } else {
            match match_config(&rest.join(" "), &cfgs) {
                Some(i) => Some(cfgs[i].clone()),
                None => {
                    let txt = format!("no config like '{}'; say 'list {} configs'", rest.join(" "), model.name);
                    self.say(&label, &txt, "failed", None);
                    return Ok(());
                }
            }
        };

        if ai {
            // Extra AI-driven car; the player stays in their own car.
            let id = crate::ai::spawn_ai(&self.mcp, &model.key, chosen.as_ref().map(|c| c.key.as_str()))?;
            let cfg = chosen.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "default config".into());
            self.say(&label, &format!("{} driving as traffic (id {})", cfg, id), "applied", None);
            let _ = self.ensure_play();
            return Ok(());
        }
        let mut args = json!({ "model": model.key, "add": add });
        if let Some(c) = &chosen {
            args["config"] = json!(c.key);
        }
        self.mcp.call("spawn", args)?;

        let cfg_name = |key: &str| cfgs.iter().find(|c| c.key == key).map(|c| c.name.clone()).unwrap_or_else(|| key.to_string());
        let mut txt = match &chosen {
            Some(c) => c.name.clone(),
            None => format!("{} (default)", cfg_name(&default)),
        };
        if chosen.is_none() {
            match self.last_config.get(&model.key) {
                Some(last) if *last != default => {
                    txt = format!(
                        "{}. Last used: {}; say 'spawn {} {}' for it, or 'list {} configs'",
                        txt,
                        cfg_name(last),
                        model.name,
                        cfg_name(last),
                        model.name
                    );
                }
                _ => txt = format!("{}. Say 'list {} configs' for others", txt, model.name),
            }
        }
        if let Some(c) = &chosen {
            self.last_config.insert(model.key.clone(), c.key.clone());
        }
        // A spawn right after a map load can leave the game on the pause screen.
        std::thread::sleep(Duration::from_millis(1000));
        let _ = self.ensure_play();
        self.say(&label, &txt, "applied", Some(self.cfg.messages.ttl_list));
        Ok(())
    }

    fn list_configs(&mut self, words: &[String]) -> Result<()> {
        let key = if words.is_empty() {
            s(&self.status()?["vehicle"]["jbeam"])
        } else {
            match self.cat.match_model(words) {
                Some((i, _, _)) => self.cat.models[i].key.clone(),
                None => {
                    self.say("Configs", &format!("vehicle '{}' not found", words.join(" ")), "failed", None);
                    return Ok(());
                }
            }
        };
        let name = self.cat.model(&key).map(|m| m.name.clone()).unwrap_or_else(|| key.clone());
        let (default, cfgs) = self.cat.configs(&self.mcp, &key)?;
        for c in &cfgs {
            println!("  {:<40} {}{}", c.name, c.key, if c.key == default { "  (default)" } else { "" });
        }
        let mut names: Vec<String> = cfgs.iter().map(|c| c.name.clone()).collect();
        let mut txt = format!("{} configs: ", cfgs.len());
        // Keep the in-game message readable; the console has the full list.
        let mut shown = 0;
        for n in names.drain(..) {
            if txt.len() + n.len() > 320 {
                break;
            }
            txt.push_str(&n);
            txt.push_str(", ");
            shown += 1;
        }
        let mut txt = txt.trim_end_matches(", ").to_string();
        if shown < cfgs.len() {
            txt.push_str(&format!(" … +{} more (see console)", cfgs.len() - shown));
        }
        self.say(&format!("{} configs", name), &txt, "applied", Some(self.cfg.messages.ttl_list));
        Ok(())
    }

    fn load_map(&mut self, words: &[String]) -> Result<()> {
        let q = words.join(" ");
        let Some(li) = self.cat.match_level(&q) else {
            self.say(&format!("Load map {}", q), "map not found; say 'list maps'", "failed", None);
            return Ok(());
        };
        let level = self.cat.levels[li].clone();
        let label = format!("Map {}", level.title);
        if self.current_level().map(|c| c.eq_ignore_ascii_case(&level.name)).unwrap_or(false) {
            self.say(&label, &self.tmpl("already", &[("state", "loaded".into())]), "already", None);
            return Ok(());
        }
        self.say(&label, "loading…", "already", Some(60.0));
        self.mcp.call("load_level", json!({ "name": level.name }))?;
        let t0 = Instant::now();
        let want = format!("/levels/{}/", level.name.to_lowercase());
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if let Ok(st) = self.status() {
                if s(&st["level"]).to_lowercase().contains(&want) && st["vehicle"].is_object() {
                    break;
                }
            }
            if t0.elapsed() > Duration::from_secs(180) {
                return Err(anyhow!("map did not finish loading"));
            }
        }
        // The main menu can stay up over the freshly loaded map; give the UI a moment, then close it.
        std::thread::sleep(Duration::from_millis(1500));
        let closed = self.ensure_play().unwrap_or(false);
        let txt = format!(
            "loaded ({:.0} s){}",
            t0.elapsed().as_secs_f64(),
            if closed { ", menu closed" } else { "" }
        );
        self.say(&label, &txt, "applied", None);
        Ok(())
    }
}

fn fmt_read(group: &str, data: &Value) -> String {
    let Some(obj) = data.as_object() else { return "no data".into() };
    let num = |k: &str| obj.get(k).and_then(|v| v.as_f64());
    let mut out: Vec<String> = vec![];
    match group {
        "temps" => {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            for k in keys {
                if let Some(v) = obj[k].as_f64() {
                    let name = match k.as_str() {
                        "watertemp" => "coolant".to_string(),
                        "oiltemp" => "oil".to_string(),
                        o => o.trim_end_matches("temp").trim_end_matches("Temp").to_string(),
                    };
                    out.push(format!("{} {:.0}°C", name, v));
                }
            }
        }
        "fuel" => {
            if let Some(f) = num("fuel") {
                out.push(format!("{:.0}%", f * 100.0));
            }
            if let (Some(v), Some(c)) = (num("fuelVolume"), num("fuelCapacity")) {
                out.push(format!("{:.0} of {:.0} L", v, c));
            }
            if num("lowfuel").unwrap_or(0.0) > 0.0 || obj.get("lowfuel").and_then(|v| v.as_bool()).unwrap_or(false) {
                out.push("LOW".into());
            }
        }
        "engine" => {
            if let Some(r) = num("rpm") {
                out.push(format!("{:.0} rpm", r));
            }
            match obj.get("engineRunning") {
                Some(Value::Bool(b)) => out.push(if *b { "running".into() } else { "off".into() }),
                Some(v) if v.as_f64().is_some() => out.push(if v.as_f64() == Some(1.0) { "running".into() } else { "off".into() }),
                _ => {}
            }
            if num("checkengine").unwrap_or(0.0) > 0.0 {
                out.push("check engine light ON".into());
            }
        }
        "speed" => {
            if let Some(v) = num("wheelspeed") {
                out.push(format!("{:.0} km/h", v * 3.6));
            }
        }
        _ => {
            for (k, v) in obj {
                out.push(format!("{} {}", k, v));
            }
        }
    }
    if out.is_empty() { "no data".into() } else { out.join(", ") }
}

fn fmt_switches(d: &Value) -> String {
    let mut parts: Vec<String> = vec![];
    for x in arr(&d["driveModes"]) {
        let modes: Vec<String> = arr(&x["modes"]).iter().map(|m| s(&m["name"])).collect();
        parts.push(format!("{} [{}] ({})", s(&x["ctrl"]), modes.join("/"), s(&x["current"])));
    }
    for x in arr(&d["devices"]) {
        let modes: Vec<String> = arr(&x["modes"]).iter().map(|m| s(m)).collect();
        parts.push(format!("{} [{}]", name_of(x), modes.join("/")));
    }
    let mut el: Vec<&str> = vec![];
    if let Some(o) = d["electrics"].as_object() {
        for (k, label) in [
            ("lights_state", "headlights"),
            ("fog", "fog lights"),
            ("hazard_enabled", "hazards"),
            ("signal_left_input", "indicators"),
            ("lightbar", "light bar"),
        ] {
            if o.contains_key(k) {
                el.push(label);
            }
        }
    }
    if !el.is_empty() {
        parts.push(el.join(", "));
    }
    let mut assists: Vec<&str> = vec![];
    for (k, n) in [("hasESC", "ESC"), ("hasTCS", "TC"), ("hasABS", "ABS")] {
        if d["assists"][k].as_f64() == Some(1.0) {
            assists.push(n);
        }
    }
    if !assists.is_empty() {
        parts.push(format!("assists: {}", assists.join("/")));
    }
    if let Some(g) = d["gearbox"].as_object() {
        parts.push(format!("gearbox {}", s(&g["type"]).replace("Gearbox", "")));
    }
    parts.join("; ")
}
