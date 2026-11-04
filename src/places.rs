//! Places on the current map (spawn points, fuel and charging stations, drag strips) and
//! what to do with them: list, satnav route, distance and compass direction, AI drive, teleport.
//! Everything is read live from the game on each command.

use anyhow::Result;
use serde_json::{json, Value};

use crate::catalog::norm;
use crate::lua::arr;
use crate::mcp::Mcp;
use crate::paint::Outcome;

#[derive(Debug, Clone)]
pub struct Place {
    pub name: String,
    /// spawnPoint | gasStation | dragstrip
    pub kind: String,
    pub pos: [f64; 3],
    pub dist: f64,
}

impl Place {
    fn kind_word(&self) -> &'static str {
        match self.kind.as_str() {
            "gasStation" if self.name.to_lowercase().contains("charging") => "charging",
            "gasStation" => "fuel",
            "dragstrip" => "drag strip",
            _ => "area",
        }
    }
}

/// Extensions a map loaded through MCP may be missing.
pub const ENSURE_EXT: &str = "for _,e in ipairs({'core_quickAccess','core_multiSpawn','gameplay_traffic','gameplay_police','gameplay_rawPois','core_groundMarkers'}) do \
    if not _G[e] then pcall(extensions.load, e) end end";

pub struct Here {
    pub places: Vec<Place>,
    pub player: Option<[f64; 3]>,
}

pub fn load(mcp: &Mcp) -> Result<Here> {
    let code = format!(
        r#"{ENSURE_EXT}
local list=(gameplay_rawPois and gameplay_rawPois.getRawPoiListByLevel(getCurrentLevelIdentifier())) or {{}}
local v=be:getPlayerVehicle(0) local pp=v and v:getPosition()
local out={{}}
for _,p in ipairs(list) do
  local t=p.data and p.data.type
  local bm=p.markerInfo and p.markerInfo.bigmapMarker
  if t and t~='mission' and bm and bm.pos then
    local n=bm.name or p.id
    local ok,tr=pcall(translateLanguage,n,n)
    if ok and type(tr)=='string' and tr~='' then n=tr end
    out[#out+1]={{name=n,kind=t,x=bm.pos.x,y=bm.pos.y,z=bm.pos.z,d=pp and (bm.pos-pp):length() or 0}}
  end
end
return jsonEncode({{places=out, player=pp and {{pp.x,pp.y,pp.z}} or nil}})"#
    );
    let v: Value = serde_json::from_str(&mcp.run_lua(&code)?)?;
    let mut places: Vec<Place> = arr(&v["places"])
        .iter()
        .filter_map(|p| {
            Some(Place {
                name: p["name"].as_str()?.to_string(),
                kind: p["kind"].as_str()?.to_string(),
                pos: [p["x"].as_f64()?, p["y"].as_f64()?, p["z"].as_f64()?],
                dist: p["d"].as_f64().unwrap_or(0.0),
            })
        })
        // "Alder Drag Strip (Rules & Setup)" is a second marker for the same place
        .filter(|p| !p.name.contains('('))
        .collect();
    places.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap_or(std::cmp::Ordering::Equal));
    let player = v["player"].as_array().and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?]));
    Ok(Here { places, player })
}

const FILL: &[&str] = &[
    "the", "a", "an", "to", "me", "us", "my", "for", "of", "is", "it", "s", "set", "satnav", "sat", "nav", "navigation",
    "gps", "route", "directions", "navigate", "navigator", "show", "how", "get", "do", "i", "we", "where", "wheres",
    "find", "far", "what", "which", "distance", "direction", "bearing", "heading", "way", "take", "drive", "teleport",
    "warp", "beam", "spawn", "at", "jump", "please", "from", "here", "away", "autopilot", "auto", "go", "can", "list",
    "places", "locations", "destinations", "spots", "there", "are", "on", "this", "map", "all", "tell", "give", "and",
];

/// Category words: (spoken, category)
fn category(w: &str) -> Option<&'static str> {
    Some(match w {
        "gas" | "fuel" | "petrol" | "diesel" | "servo" | "service" | "filling" => "fuel",
        "charging" | "charger" | "chargers" | "ev" | "electric" | "recharge" => "charging",
        "drag" | "dragstrip" => "drag strip",
        "spawn" | "spawnpoint" | "area" | "areas" => "area",
        _ => return None,
    })
}

/// Pick the place a request means. Returns (place, how it was matched).
pub fn find<'a>(here: &'a Here, words: &[String]) -> Option<&'a Place> {
    let nearest = words.iter().any(|w| ["nearest", "closest", "nearby", "near"].contains(&w.as_str()));
    let q: Vec<String> = words
        .iter()
        .filter(|w| !FILL.contains(&w.as_str()))
        .filter(|w| !["nearest", "closest", "nearby", "near"].contains(&w.as_str()))
        .cloned()
        .collect();
    if q.is_empty() {
        return None;
    }
    let qn = q.join(" ");
    // 1. exact name ("service station" is also a spawn point on West Coast)
    let exact = here.places.iter().find(|p| norm(&p.name) == qn);
    if exact.is_some() && !nearest {
        return exact;
    }
    // 2. category ("nearest gas station", "a charging station")
    let cat = q.iter().find_map(|w| category(w));
    if let Some(c) = cat {
        let extra: Vec<&String> = q
            .iter()
            .filter(|w| category(w).is_none() && !["station", "stations", "strip", "point", "points"].contains(&w.as_str()))
            .collect();
        if extra.is_empty() || nearest {
            // places are sorted by distance
            if let Some(p) = here.places.iter().find(|p| p.kind_word() == c) {
                return Some(p);
            }
        }
    }
    // 3. fuzzy name match; equal scores go to the nearest
    let mut best: Option<(&Place, f64)> = None;
    for p in &here.places {
        let pn = norm(&p.name);
        let toks: Vec<&str> = pn.split(' ').collect();
        let overlap = q.iter().filter(|w| toks.iter().any(|t| t == w || strsim::normalized_levenshtein(t, w) >= 0.8)).count() as f64
            / q.len() as f64;
        let lev = strsim::normalized_levenshtein(&pn.replace(' ', ""), &qn.replace(' ', ""));
        // A near-exact match with the spaces removed counts on its own ("china town").
        let score = (overlap * 0.7 + lev * 0.3).max(if lev >= 0.85 { lev } else { 0.0 });
        if best.map(|(_, s)| score > s + 1e-9).unwrap_or(true) {
            best = Some((p, score));
        }
    }
    best.filter(|(_, s)| *s >= 0.5).map(|(p, _)| p)
}

pub fn fmt_dist(m: f64) -> String {
    if m < 1000.0 {
        format!("{:.0} m", (m / 10.0).round() * 10.0)
    } else {
        format!("{:.1} km", m / 1000.0)
    }
}

/// Compass direction from a to b (BeamNG: +x east, +y north).
pub fn compass(a: [f64; 3], b: [f64; 3]) -> &'static str {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    if dx.abs() < 1.0 && dy.abs() < 1.0 {
        return "here";
    }
    let deg = dx.atan2(dy).to_degrees().rem_euclid(360.0);
    const P: [&str; 8] = ["north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west"];
    P[(((deg + 22.5) / 45.0) as usize) % 8]
}

fn where_text(here: &Here, p: &Place) -> String {
    match here.player {
        Some(pp) => format!("{} {}", fmt_dist(p.dist), compass(pp, p.pos)),
        None => String::new(),
    }
}

fn not_found(label: &str, words: &[String], here: &Here) -> Outcome {
    let q: Vec<String> = words.iter().filter(|w| !FILL.contains(&w.as_str())).cloned().collect();
    let text = if here.places.is_empty() {
        "this map has no named places".to_string()
    } else {
        format!("no place like '{}'; say 'list places'", q.join(" "))
    };
    Outcome { label: label.into(), text, status: "failed" }
}

/// "list places", "list gas stations"
pub fn list(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let here = load(mcp)?;
    let cat = words.iter().find_map(|w| category(w));
    let shown: Vec<&Place> = here.places.iter().filter(|p| cat.map(|c| p.kind_word() == c).unwrap_or(true)).collect();
    for p in &shown {
        println!("  {:<34} {:<10} {}", p.name, p.kind_word(), where_text(&here, p));
    }
    if shown.is_empty() {
        return Ok(Outcome { label: "Places".into(), text: "none on this map".into(), status: "failed" });
    }
    // Group names by kind for the in-game message, nearest first.
    let mut groups: Vec<(&str, Vec<String>)> = vec![];
    for p in &shown {
        let k = p.kind_word();
        let entry = format!("{} ({})", p.name, fmt_dist(p.dist));
        match groups.iter_mut().find(|(g, _)| *g == k) {
            Some((_, v)) => v.push(entry),
            None => groups.push((k, vec![entry])),
        }
    }
    let text = groups
        .iter()
        .map(|(k, v)| {
            let title = match *k {
                "area" => "Areas",
                "fuel" => "Fuel",
                "charging" => "Charging",
                _ => "Drag strips",
            };
            format!("{}: {}", title, v.join(", "))
        })
        .collect::<Vec<_>>()
        .join(" · ");
    Ok(Outcome { label: format!("Places ({})", shown.len()), text, status: "applied" })
}

fn set_route(mcp: &Mcp, p: &Place) -> Result<Option<f64>> {
    let code = format!(
        "{ENSURE_EXT} core_groundMarkers.setPath(vec3({},{},{}), {{clearPathOnReachingTarget=true}}) return 'ok'",
        p.pos[0], p.pos[1], p.pos[2]
    );
    mcp.run_lua(&code)?;
    // The route length is known a frame or two later.
    std::thread::sleep(std::time::Duration::from_millis(400));
    let len = mcp
        .run_lua("return tostring(core_groundMarkers.getPathLength and core_groundMarkers.getPathLength() or '')")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|l| *l > 0.0);
    Ok(len)
}

/// "navigate to X", "where is X", "find the nearest gas station"
pub fn navigate(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let here = load(mcp)?;
    let Some(p) = find(&here, words) else { return Ok(not_found("Satnav", words, &here)) };
    let road = set_route(mcp, p)?;
    let mut text = format!("route set: {}, {}", where_text(&here, p), p.kind_word());
    if let Some(r) = road {
        text = format!("route set: {} by road ({} {})", fmt_dist(r), fmt_dist(p.dist), here.player.map(|pp| compass(pp, p.pos)).unwrap_or(""));
    }
    Ok(Outcome { label: format!("Satnav {}", p.name), text, status: "applied" })
}

/// "how far is the nearest service station", "what direction is the quarry"
pub fn distance(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let here = load(mcp)?;
    let Some(p) = find(&here, words) else { return Ok(not_found("Distance", words, &here)) };
    let Some(pp) = here.player else {
        return Ok(Outcome { label: format!("Distance to {}", p.name), text: "no player vehicle".into(), status: "failed" });
    };
    Ok(Outcome {
        label: format!("Distance to {}", p.name),
        text: format!("{} {} (straight line)", fmt_dist(p.dist), compass(pp, p.pos)),
        status: "applied",
    })
}

pub fn clear(mcp: &Mcp) -> Result<Outcome> {
    let had = mcp.run_lua("return tostring(core_groundMarkers and core_groundMarkers.currentlyHasTarget and core_groundMarkers.currentlyHasTarget() or false)")?;
    if had.trim() != "true" {
        return Ok(Outcome { label: "Clear satnav".into(), text: "no route set".into(), status: "already" });
    }
    mcp.run_lua("core_groundMarkers.setPath(nil) return 'ok'")?;
    Ok(Outcome { label: "Clear satnav".into(), text: "route cleared".into(), status: "applied" })
}

fn drive(mcp: &Mcp, pos: [f64; 3]) -> Result<()> {
    mcp.call("drive_to", json!({ "pos": { "x": pos[0], "y": pos[1], "z": pos[2] }, "driveInLane": "on" }))?;
    Ok(())
}

/// "autopilot on" drives the current satnav route; "autopilot off" hands back control.
pub fn autopilot(mcp: &Mcp, on: bool) -> Result<Outcome> {
    let label = format!("Autopilot {}", if on { "on" } else { "off" });
    if !on {
        mcp.call("set_ai", json!({ "mode": "disabled" }))?;
        return Ok(Outcome { label, text: "you have control".into(), status: "applied" });
    }
    let t = mcp.run_lua(
        "if core_groundMarkers and core_groundMarkers.currentlyHasTarget and core_groundMarkers.currentlyHasTarget() then \
         local p=core_groundMarkers.getTargetPos() if p then return jsonEncode({p.x,p.y,p.z}) end end return 'none'",
    )?;
    let target: Option<[f64; 3]> = serde_json::from_str::<Vec<f64>>(t.trim()).ok().filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
    match target {
        Some(pos) => {
            drive(mcp, pos)?;
            Ok(Outcome { label, text: "driving the satnav route".into(), status: "applied" })
        }
        None => {
            // No route: cruise as traffic.
            mcp.call("set_ai", json!({ "mode": "traffic" }))?;
            Ok(Outcome { label, text: "no satnav route, so driving as traffic; say 'take me to …' for a destination".into(), status: "applied" })
        }
    }
}

/// "take me to X": route on the satnav and the AI drives it.
pub fn drive_to(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let here = load(mcp)?;
    let Some(p) = find(&here, words) else { return Ok(not_found("Drive to", words, &here)) };
    let road = set_route(mcp, p)?;
    drive(mcp, p.pos)?;
    let dist = road.map(|r| format!("{} by road", fmt_dist(r))).unwrap_or_else(|| where_text(&here, p));
    Ok(Outcome { label: format!("Drive to {}", p.name), text: format!("on the way, {}; say 'autopilot off' to take over", dist), status: "applied" })
}

/// "teleport to X", "spawn at X": instant move (repairs the car too).
pub fn teleport(mcp: &Mcp, words: &[String]) -> Result<Outcome> {
    let here = load(mcp)?;
    let Some(p) = find(&here, words) else { return Ok(not_found("Teleport", words, &here)) };
    mcp.call("set_position", json!({ "pos": { "x": p.pos[0], "y": p.pos[1], "z": p.pos[2] + 0.5 } }))?;
    Ok(Outcome { label: format!("Teleport to {}", p.name), text: format!("moved {}", fmt_dist(p.dist)), status: "applied" })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compass_points() {
        let o = [0.0, 0.0, 0.0];
        assert_eq!(compass(o, [0.0, 100.0, 0.0]), "north");
        assert_eq!(compass(o, [100.0, 0.0, 0.0]), "east");
        assert_eq!(compass(o, [0.0, -100.0, 0.0]), "south");
        assert_eq!(compass(o, [-100.0, 0.0, 0.0]), "west");
        assert_eq!(compass(o, [100.0, 100.0, 0.0]), "north-east");
        assert_eq!(compass(o, [-100.0, -90.0, 0.0]), "south-west");
        assert_eq!(fmt_dist(345.0), "350 m");
        assert_eq!(fmt_dist(1737.0), "1.7 km");
    }
    fn here() -> Here {
        let mk = |n: &str, k: &str, d: f64| Place { name: n.into(), kind: k.into(), pos: [d, 0.0, 0.0], dist: d };
        Here {
            places: vec![
                mk("Industrial Area", "spawnPoint", 0.0),
                mk("Nodeoline Gas Station", "gasStation", 228.0),
                mk("Chinatown", "spawnPoint", 449.0),
                mk("Apex Charging Station", "gasStation", 484.0),
                mk("Drag Strip", "spawnPoint", 580.0),
                mk("Alder Drag Strip", "dragstrip", 854.0),
                mk("Service Station", "spawnPoint", 1707.0),
                mk("Trilobite Service Station", "gasStation", 1737.0),
            ],
            player: Some([0.0, 0.0, 0.0]),
        }
    }
    fn w(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }
    #[test]
    fn matching() {
        let h = here();
        let name = |s: &str| find(&h, &w(s)).map(|p| p.name.clone()).unwrap_or_default();
        assert_eq!(name("the nearest gas station"), "Nodeoline Gas Station");
        assert_eq!(name("nearest service station"), "Nodeoline Gas Station");
        assert_eq!(name("the service station"), "Service Station");
        assert_eq!(name("nearest charging station"), "Apex Charging Station");
        assert_eq!(name("chinatown"), "Chinatown");
        assert_eq!(name("china town"), "Chinatown");
        assert_eq!(name("trilobite"), "Trilobite Service Station");
        assert_eq!(name("the drag strip"), "Drag Strip");
        assert_eq!(name("nearest drag strip"), "Alder Drag Strip");
        assert_eq!(name("narnia"), "");
    }
}
