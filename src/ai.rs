//! Traffic/police spawning and per-vehicle AI behaviour, with vehicles picked by
//! description: "make the white van chase me", "police follow me", "stop the red taxi",
//! "all traffic drive aggressively", "autopilot on".

use anyhow::Result;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use crate::catalog::norm;
use crate::lua::arr;
use crate::mcp::Mcp;
use crate::paint::{canon_colour, classify, Outcome};

#[derive(Debug, Clone)]
struct Veh {
    id: i64,
    name: String,
    tokens: Vec<String>,
    colour: Option<&'static str>,
    player: bool,
    role: String,
    pos: (f64, f64),
}

fn vehicles(mcp: &Mcp) -> Result<Vec<Veh>> {
    let code = r#"
if not core_quickAccess then extensions.load('core_quickAccess') end
local pid=be:getPlayerVehicleID(0)
local td=(gameplay_traffic and gameplay_traffic.getTrafficData()) or {}
local out={}
for i=0,be:getObjectCount()-1 do
  local v=be:getObject(i) local id=v:getID()
  local name=(core_quickAccess and core_quickAccess.getVehicleName(v)) or v.JBeam
  local m=core_vehicles.getModel(v.JBeam)
  local body=(m and m.model and m.model['Body Style']) or ''
  local typ=(m and m.model and m.model.Type) or ''
  local pc=v.partConfig local key=(type(pc)=='string' and pc:match('([^/]+)%.pc$')) or ''
  if v.JBeam=='simple_traffic' and key~='' then
    local real=core_vehicles.getModel(key:match('^([%a%d]+)') or '')
    if real and real.model then body=real.model['Body Style'] or body typ=real.model.Type or typ end
  end
  local vd=core_vehicle_manager.getVehicleData(id)
  local c=vd and vd.config and vd.config.paints and vd.config.paints[1] and vd.config.paints[1].baseColor
  local role=(td[id] and td[id].role and td[id].role.name) or ''
  local p=v:getPosition()
  out[#out+1]={id=id,name=name,cfg=key,body=body,type=typ,role=role,colour=c,player=(id==pid),x=p.x,y=p.y}
end
return jsonEncode(out)"#;
    let v: Value = serde_json::from_str(&mcp.run_lua(code)?)?;
    Ok(arr(&v)
        .iter()
        .map(|x| {
            let name = x["name"].as_str().unwrap_or("").replace("Simplified Traffic Vehicles ", "");
            let role = x["role"].as_str().unwrap_or("").to_string();
            let mut desc = format!(
                "{} {} {} {} {}",
                name,
                x["body"].as_str().unwrap_or(""),
                x["type"].as_str().unwrap_or(""),
                role,
                x["cfg"].as_str().unwrap_or("").replace('_', " ")
            );
            if role == "police" || name.to_lowercase().contains("police") {
                desc.push_str(" police");
            }
            let colour = x["colour"].as_array().and_then(|a| {
                Some(classify([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?]))
            });
            Veh {
                id: x["id"].as_i64().unwrap_or(0),
                name,
                tokens: norm(&desc).split(' ').map(String::from).collect(),
                colour,
                player: x["player"].as_bool().unwrap_or(false),
                role,
                pos: (x["x"].as_f64().unwrap_or(0.0), x["y"].as_f64().unwrap_or(0.0)),
            }
        })
        .collect())
}

fn canon_desc(w: &str) -> &str {
    match w {
        "cop" | "cops" | "policeman" | "cruiser" | "interceptor" => "police",
        "ute" | "utes" | "pickups" => "pickup",
        "lorry" | "trucks" => "truck",
        "cab" | "taxis" | "cabs" => "taxi",
        "vans" => "van",
        "wagons" | "estate" => "wagon",
        "sedans" | "saloon" => "sedan",
        other => other,
    }
}

fn number(words: &[String]) -> Option<i64> {
    for w in words {
        if let Ok(n) = w.parse::<i64>() {
            return Some(n);
        }
        let n = match w.as_str() {
            "a" | "an" | "one" | "single" => Some(1),
            "two" | "couple" | "pair" => Some(2),
            "three" | "few" => Some(3),
            "four" => Some(4),
            "five" => Some(5),
            "six" => Some(6),
            "seven" => Some(7),
            "eight" => Some(8),
            "nine" => Some(9),
            "ten" => Some(10),
            _ => None,
        };
        if n.is_some() {
            return n;
        }
    }
    None
}

fn traffic_count(mcp: &Mcp) -> usize {
    mcp.run_lua(
        "local n=0 local pid=be:getPlayerVehicleID(0) for id in pairs((gameplay_traffic and gameplay_traffic.getTrafficData()) or {}) do if id~=pid then n=n+1 end end return tostring(n)",
    )
    .ok()
    .and_then(|s| s.trim().parse().ok())
    .unwrap_or(0)
}

/// "spawn traffic", "add 2 police cars", "remove traffic".
pub fn traffic(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let has = |w: &str| words.iter().any(|x| x == w);
    let any = |ws: &[&str]| ws.iter().any(|w| has(*w));
    let police = any(&["police", "cop", "cops", "cruiser", "cruisers"]);
    let label = if police { "Police".to_string() } else { "Traffic".to_string() };
    let before = traffic_count(mcp);

    if any(&["remove", "clear", "delete", "despawn", "stop", "no", "off", "kill", "disable"]) {
        mcp.run_lua("gameplay_traffic.deleteVehicles() return 'ok'")?;
        std::thread::sleep(Duration::from_millis(800));
        let after = traffic_count(mcp);
        return Ok(Outcome { label, text: format!("removed {} vehicles", before.saturating_sub(after)), status: "applied" });
    }

    let n = number(words);
    let code = if police {
        let n = n.unwrap_or(1).clamp(1, 10);
        format!(
            "local g=gameplay_traffic_trafficUtils.createPoliceGroup({n}) \
             return tostring(gameplay_traffic.setupTraffic({n},{{vehGroup=g,keepCurrent=true}}))"
        )
    } else {
        let n = n.unwrap_or(-1).clamp(-1, 30);
        format!("return tostring(gameplay_traffic.setupTraffic({n},{{keepCurrent=true}}))")
    };
    let ok = mcp.run_lua(&code)?;
    if ok.trim() != "true" {
        return Ok(Outcome { label, text: "the game refused to spawn traffic (no road network on this map?)".into(), status: "failed" });
    }
    // Vehicles spawn over the next frames; wait for the count to rise.
    let t0 = Instant::now();
    let mut after = before;
    while t0.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(1000));
        after = traffic_count(mcp);
        if after > before && (Instant::now() - t0) > Duration::from_secs(3) {
            break;
        }
    }
    let added = after.saturating_sub(before);
    if added == 0 {
        return Ok(Outcome { label, text: "nothing spawned yet".into(), status: "partial" });
    }
    Ok(Outcome {
        label,
        text: format!("added {} {}; {} AI vehicles now", added, if police { "police cars" } else { "vehicles" }, after),
        status: "applied",
    })
}

#[derive(Debug, Default)]
struct Behaviour {
    mode: Option<&'static str>,
    aggression: Option<f64>,
    speed_ms: Option<f64>,
    describe: Vec<String>,
}

const BEHAVIOUR_WORDS: &[&str] = &[
    "chase", "chasing", "pursue", "hunt", "ram", "follow", "following", "tail", "flee", "escape", "run", "away", "stop",
    "halt", "park", "pull", "over", "normally", "normal", "obey", "random", "randomly", "wander", "roam", "cruise",
    "manual", "disable", "autopilot", "aggressive", "aggressively", "crazy", "calm", "careful", "carefully", "gently",
    "gentle", "safely", "slow", "slowly", "fast", "speed", "limit", "kmh", "km", "kph", "mph", "drive", "driving",
    "around", "like", "traffic", "ai", "off", "on", "control", "lane", "lanes",
];
const FILL: &[&str] = &[
    "make", "the", "a", "an", "tell", "get", "have", "let", "please", "to", "me", "us", "my", "i", "car", "cars",
    "vehicle", "vehicles", "one", "that", "this", "set", "start", "begin", "and", "at", "with", "it", "them",
];

fn parse_behaviour(words: &[String]) -> Behaviour {
    let has = |w: &str| words.iter().any(|x| x == w);
    let any = |ws: &[&str]| ws.iter().any(|w| has(*w));
    let mut b = Behaviour::default();
    if any(&["chase", "chasing", "pursue", "hunt", "ram"]) {
        b.mode = Some("chase");
        b.describe.push("chasing you".into());
    } else if any(&["follow", "following", "tail"]) {
        b.mode = Some("follow");
        b.describe.push("following you".into());
    } else if has("flee") || has("escape") || (has("run") && has("away")) {
        b.mode = Some("flee");
        b.describe.push("fleeing from you".into());
    } else if any(&["stop", "halt", "park"]) || (has("pull") && has("over")) {
        b.mode = Some("stop");
        b.describe.push("stopping".into());
    } else if any(&["manual", "disable"]) || (has("autopilot") && has("off")) || (has("ai") && has("off")) {
        b.mode = Some("disabled");
        b.describe.push("AI off".into());
    } else if any(&["random", "randomly", "wander", "roam", "cruise"]) {
        b.mode = Some("random");
        b.describe.push("driving around randomly".into());
    } else if any(&["normally", "normal", "obey", "autopilot", "lane", "lanes"]) || (has("drive") && has("like") && has("traffic")) {
        b.mode = Some("traffic");
        b.describe.push("driving as traffic".into());
    }
    if any(&["aggressive", "aggressively", "crazy", "fast"]) {
        b.aggression = Some(1.0);
        b.describe.push("aggressive".into());
    } else if any(&["calm", "careful", "carefully", "gently", "gentle", "safely", "slow", "slowly"]) {
        b.aggression = Some(0.3);
        b.describe.push("calm".into());
    }
    if let Some(i) = words.iter().position(|w| w.parse::<f64>().is_ok()) {
        let n: f64 = words[i].parse().unwrap_or(0.0);
        let unit = words.get(i + 1).map(|s| s.as_str()).unwrap_or("");
        let ms = match unit {
            "mph" | "miles" => Some(n * 0.447),
            "kmh" | "km" | "kph" | "kilometres" | "kilometers" => Some(n / 3.6),
            _ if has("speed") || has("limit") => Some(n / 3.6),
            _ => None,
        };
        if let Some(ms) = ms {
            b.speed_ms = Some(ms);
            b.describe.push(format!("max {:.0} km/h", ms * 3.6));
        }
    }
    b
}

/// Per-vehicle AI behaviour by description.
pub fn command(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let has = |w: &str| words.iter().any(|x| x == w);
    let b = parse_behaviour(words);
    let label_words: Vec<String> = words.iter().filter(|w| !FILL.contains(&w.as_str())).cloned().collect();
    let label = format!("AI {}", label_words.join(" "));
    if b.mode.is_none() && b.aggression.is_none() && b.speed_ms.is_none() {
        return Ok(Outcome { label, text: "which behaviour? chase, follow, flee, stop, normal, random, aggressive, calm".into(), status: "failed" });
    }

    let all = vehicles(mcp)?;
    let player = all.iter().find(|v| v.player).cloned();
    let pid = player.as_ref().map(|p| p.id).unwrap_or(0);
    let ppos = player.as_ref().map(|p| p.pos).unwrap_or((0.0, 0.0));

    // Subject: the player's own car ("autopilot on", "AI drive my car")
    let self_drive = has("autopilot") || (has("my") && has("car")) || (has("ai") && has("drive") && has("me"));
    let everyone = ["all", "everyone", "everybody", "every"].iter().any(|w| has(*w));
    let colour = words.iter().find_map(|w| canon_colour(w));
    let desc: Vec<String> = words
        .iter()
        .filter(|w| !FILL.contains(&w.as_str()) && !BEHAVIOUR_WORDS.contains(&w.as_str()) && canon_colour(w).is_none())
        .filter(|w| !["all", "everyone", "everybody", "every", "other", "others"].contains(&w.as_str()))
        .filter(|w| w.parse::<f64>().is_err())
        .map(|w| canon_desc(w).to_string())
        .collect();

    let mut targets: Vec<&Veh> = if self_drive {
        all.iter().filter(|v| v.player).collect()
    } else {
        all.iter()
            .filter(|v| !v.player)
            .filter(|v| colour.map(|c| v.colour == Some(c)).unwrap_or(true))
            .filter(|v| {
                desc.iter().all(|w| {
                    v.tokens.iter().any(|t| t == w || (w.len() >= 3 && t.starts_with(w.as_str())) || (t.len() >= 4 && w.starts_with(t.as_str())))
                })
            })
            .collect()
    };
    let dist = |v: &Veh| (v.pos.0 - ppos.0).powi(2) + (v.pos.1 - ppos.1).powi(2);
    targets.sort_by(|a, b| dist(*a).partial_cmp(&dist(*b)).unwrap_or(std::cmp::Ordering::Equal));
    let described = !desc.is_empty() || colour.is_some();
    if targets.is_empty() {
        let what = format!("{}{}", colour.map(|c| format!("{} ", c)).unwrap_or_default(), if desc.is_empty() { "vehicle".into() } else { desc.join(" ") });
        return Ok(Outcome { label, text: format!("no {} found", what), status: "failed" });
    }
    if !self_drive && !everyone && !described && targets.len() > 1 {
        return Ok(Outcome { label, text: format!("which one? {} AI vehicles here; describe it, e.g. 'the white van'", targets.len()), status: "failed" });
    }
    if !everyone && !self_drive {
        targets.truncate(1); // the nearest match
    }

    // Police chasing the player: use the game's pursuit system (sirens, roadblocks).
    let all_police = targets.iter().all(|v| v.role == "police");
    let ids: Vec<String> = targets.iter().map(|v| v.id.to_string()).collect();
    if all_police && b.mode == Some("chase") {
        mcp.run_lua(&format!("gameplay_police.setPursuitMode(1, {}, {{{}}}) return 'ok'", pid, ids.join(",")))?;
    } else {
        if all_police && b.mode.is_some() {
            let _ = mcp.run_lua(&format!("gameplay_police.setPursuitMode(0, {}, {{{}}}) return 'ok'", pid, ids.join(",")));
        }
        for v in &targets {
            let mut args = json!({ "id": v.id });
            if let Some(m) = b.mode {
                args["mode"] = json!(m);
                if matches!(m, "chase" | "follow" | "flee") {
                    args["targetObjectID"] = json!(pid);
                }
            }
            if let Some(a) = b.aggression {
                args["aggression"] = json!(a);
            }
            if let Some(s) = b.speed_ms {
                args["routeSpeed"] = json!(s);
                args["speedMode"] = json!("limit");
            }
            mcp.call("set_ai", args)?;
        }
    }

    let who = if self_drive {
        "your car".to_string()
    } else if targets.len() == 1 {
        let v = targets[0];
        format!("{}{}", v.colour.map(|c| format!("{} ", c)).unwrap_or_default(), v.name)
    } else {
        format!("{} vehicles", targets.len())
    };
    Ok(Outcome { label, text: format!("{}: {}", who, b.describe.join(", ")), status: "applied" })
}

/// Spawn a model as an extra AI car without taking the player out of their own car.
pub fn spawn_ai(mcp: &Mcp, model: &str, config: Option<&str>) -> Result<i64> {
    let pid: i64 = mcp.call("get_player_vehicle_id", json!({}))?.trim().parse().unwrap_or(0);
    let mut args = json!({ "model": model, "add": true });
    if let Some(c) = config {
        args["config"] = json!(c);
    }
    mcp.call("spawn", args)?;
    std::thread::sleep(Duration::from_millis(2500));
    let new_id: i64 = mcp.call("get_player_vehicle_id", json!({}))?.trim().parse().unwrap_or(0);
    if pid != 0 && new_id != pid {
        mcp.run_lua(&format!("local p=getObjectByID({}) if p then be:enterVehicle(0,p) end return 'ok'", pid))?;
    }
    if new_id != 0 && new_id != pid {
        mcp.call("set_ai", json!({ "id": new_id, "mode": "traffic" }))?;
    }
    Ok(new_id)
}
