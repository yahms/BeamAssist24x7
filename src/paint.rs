//! Repainting vehicles: "make the white sunburst black", "paint my car fire red",
//! "change the colour of the pigeon to matte black".
//! Vehicles are found by model and/or their current body colour; the new paint is the
//! model's own factory paint when one fits, otherwise a custom paint.

use anyhow::Result;
use serde_json::Value;

use crate::catalog::{norm, Catalog};
use crate::lua::{arr, lua_str};
use crate::mcp::Mcp;

/// Canonical colour for a spoken word.
pub fn canon_colour(w: &str) -> Option<&'static str> {
    Some(match w {
        "black" => "black",
        "white" => "white",
        "grey" | "gray" | "silver" => "grey",
        "red" | "maroon" | "burgundy" | "crimson" => "red",
        "orange" => "orange",
        "yellow" | "gold" | "golden" => "yellow",
        "green" => "green",
        "cyan" | "teal" | "turquoise" | "aqua" => "cyan",
        "blue" | "navy" => "blue",
        "purple" | "violet" => "purple",
        "pink" | "magenta" => "pink",
        "brown" | "tan" | "beige" | "bronze" => "brown",
        _ => return None,
    })
}

/// Fallback custom paints when the model has nothing in that colour.
fn basic_rgb(c: &str) -> [f64; 3] {
    match c {
        "black" => [0.01, 0.01, 0.01],
        "white" => [0.85, 0.85, 0.85],
        "grey" => [0.35, 0.35, 0.36],
        "red" => [0.7, 0.03, 0.03],
        "orange" => [0.9, 0.3, 0.02],
        "yellow" => [0.9, 0.7, 0.02],
        "green" => [0.05, 0.45, 0.08],
        "cyan" => [0.02, 0.55, 0.6],
        "blue" => [0.03, 0.15, 0.7],
        "purple" => [0.3, 0.05, 0.5],
        "pink" => [0.9, 0.3, 0.5],
        _ => [0.3, 0.15, 0.05], // brown
    }
}

/// Name the colour of a paint baseColor (r, g, b in 0..1).
pub fn classify(rgb: [f64; 3]) -> &'static str {
    let [r, g, b] = rgb;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let chroma = mx - mn;
    let v = mx;
    let s = if mx > 0.0 { chroma / mx } else { 0.0 };
    if v < 0.1 {
        return "black";
    }
    // Dark colours exaggerate saturation, so use absolute chroma too.
    if chroma < 0.08 || s < 0.22 {
        return if v >= 0.65 { "white" } else if v < 0.2 { "black" } else { "grey" };
    }
    let mut h = if mx == r {
        60.0 * (((g - b) / chroma) % 6.0)
    } else if mx == g {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    match h {
        h if !(15.0..330.0).contains(&h) => "red",
        h if h >= 300.0 => if v < 0.5 { "red" } else { "pink" },
        h if h < 40.0 => if v < 0.5 { "brown" } else { "orange" },
        h if h < 70.0 => if v < 0.4 { "brown" } else { "yellow" },
        h if h < 170.0 => "green",
        h if h < 200.0 => "cyan",
        h if h < 260.0 => "blue",
        _ => "purple",
    }
}

fn rgb(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}

struct Veh {
    id: i64,
    jbeam: String,
    player: bool,
    paints: Vec<[f64; 3]>,
    pos: (f64, f64),
}

fn vehicles(mcp: &Mcp) -> Result<Vec<Veh>> {
    let code = "local out={} local pid=be:getPlayerVehicleID(0) \
        for i=0,be:getObjectCount()-1 do local v=be:getObject(i) local id=v:getID() \
        local vd=core_vehicle_manager.getVehicleData(id) local p={} \
        if vd and vd.config and vd.config.paints then for k,pp in ipairs(vd.config.paints) do p[k]=pp.baseColor end end \
        local pos=v:getPosition() out[#out+1]={id=id,jbeam=v.JBeam,player=(id==pid),paints=p,x=pos.x,y=pos.y} end \
        return jsonEncode(out)";
    let v: Value = serde_json::from_str(&mcp.run_lua(code)?)?;
    Ok(arr(&v)
        .iter()
        .map(|x| Veh {
            id: x["id"].as_i64().unwrap_or(0),
            jbeam: x["jbeam"].as_str().unwrap_or("").to_string(),
            player: x["player"].as_bool().unwrap_or(false),
            paints: arr(&x["paints"]).iter().filter_map(|p| rgb(p)).collect(),
            pos: (x["x"].as_f64().unwrap_or(0.0), x["y"].as_f64().unwrap_or(0.0)),
        })
        .collect())
}

/// (name, rgb) factory paints of a model.
fn factory_paints(mcp: &Mcp, jbeam: &str) -> Result<Vec<(String, [f64; 3])>> {
    let code = format!(
        "local t={{}} local m=core_vehicles.getModel({}) \
         for n,p in pairs((m and m.model and m.model.paints) or {{}}) do t[#t+1]={{name=n,c=p.baseColor}} end \
         return jsonEncode(t)",
        lua_str(jbeam)
    );
    let v: Value = serde_json::from_str(&mcp.run_lua(&code)?)?;
    Ok(arr(&v)
        .iter()
        .filter_map(|p| Some((p["name"].as_str()?.to_string(), rgb(&p["c"])?)))
        .collect())
}

const FILL: &[&str] = &[
    "make", "paint", "respray", "repaint", "the", "my", "car", "it", "colour", "color", "recolour", "recolor", "of",
    "change", "vehicle", "please", "this", "that", "one", "a", "an", "its", "set",
];

pub struct Outcome {
    pub label: String,
    pub text: String,
    pub status: &'static str,
}

fn fail(label: String, text: String) -> Outcome {
    Outcome { label, text, status: "failed" }
}

pub fn run(mcp: &Mcp, cat: &Catalog, words: &[String]) -> Result<Outcome> {
    let words: Vec<String> = words.iter().filter(|w| !FILL.contains(&w.as_str())).cloned().collect();
    let label = format!("Paint {}", words.join(" "));

    // Split into subject ("white sunburst") and target ("black", "fire red").
    let (subject, target): (Vec<String>, Vec<String>) = match words.iter().position(|w| w == "to" || w == "in") {
        Some(i) => (words[..i].to_vec(), words[i + 1..].to_vec()),
        None => {
            // Target is the trailing colour phrase: the last colour word plus any preceding
            // non-colour, non-model word ("matte black", "fire red").
            let Some(last) = words.iter().rposition(|w| canon_colour(w).is_some()) else {
                // No colour word: assume the whole tail names a paint ("black diamond", "sunny flare").
                return Ok(paint_named(mcp, cat, label, &[], &words)?);
            };
            let mut start = last;
            if last > 0 && canon_colour(&words[last - 1]).is_none() && cat.match_model(&words[last - 1..last]).is_none() {
                start = last - 1;
            }
            // "black diamond": colour word followed by more words belongs to the target too.
            (words[..start].to_vec(), words[start..].to_vec())
        }
    };
    if target.is_empty() {
        return Ok(fail(label, "which colour?".into()));
    }
    paint_named(mcp, cat, label, &subject, &target)
}

fn paint_named(mcp: &Mcp, cat: &Catalog, label: String, subject: &[String], target: &[String]) -> Result<Outcome> {
    // --- which vehicle
    let subj_colour = subject.iter().find_map(|w| canon_colour(w));
    let model_words: Vec<String> = subject.iter().filter(|w| canon_colour(w).is_none()).cloned().collect();
    let model = if model_words.is_empty() { None } else { cat.match_model(&model_words).map(|(i, _, _)| cat.models[i].clone()) };
    if !model_words.is_empty() && model.is_none() {
        return Ok(fail(label, format!("no vehicle like '{}'", model_words.join(" "))));
    }
    let all = vehicles(mcp)?;
    let player_pos = all.iter().find(|v| v.player).map(|v| v.pos).unwrap_or((0.0, 0.0));
    let mut cands: Vec<&Veh> = all
        .iter()
        .filter(|v| model.as_ref().map(|m| m.key == v.jbeam).unwrap_or(true))
        .filter(|v| subj_colour.map(|c| v.paints.first().map(|p| classify(*p) == c).unwrap_or(false)).unwrap_or(true))
        .collect();
    if model.is_none() && subj_colour.is_none() {
        cands.retain(|v| v.player); // "paint my car red"
    }
    if cands.is_empty() {
        let what = format!(
            "{}{}",
            subj_colour.map(|c| format!("{} ", c)).unwrap_or_default(),
            model.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| "vehicle".into())
        );
        return Ok(fail(label, format!("no {} found", what)));
    }
    // Prefer the player's vehicle, then the nearest one.
    cands.sort_by(|a, b| {
        let da = (a.pos.0 - player_pos.0).powi(2) + (a.pos.1 - player_pos.1).powi(2);
        let db = (b.pos.0 - player_pos.0).powi(2) + (b.pos.1 - player_pos.1).powi(2);
        b.player.cmp(&a.player).then(da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal))
    });
    let veh = cands[0];
    let more = cands.len() - 1;
    let vname = cat.model(&veh.jbeam).map(|m| m.name.clone()).unwrap_or_else(|| veh.jbeam.clone());
    let old = veh.paints.first().map(|p| classify(*p)).unwrap_or("?");

    // --- which paint
    let tq = target.join(" ");
    let want_colour = target.iter().rev().find_map(|w| canon_colour(w));
    let finish_words = ["matte", "chrome", "metallic", "pearl"];
    let asked_finish: Vec<&str> = finish_words.iter().copied().filter(|f| target.iter().any(|w| w.as_str() == *f)).collect();
    let factory = factory_paints(mcp, &veh.jbeam)?;
    let tn = norm(&tq);
    let pick = factory
        .iter()
        // 1. exact paint name ("fire red", "sunny flare")
        .find(|(n, _)| norm(n) == tn)
        // 2. names containing every spoken word ("black" -> "Black Diamond", "Matte Black")
        .or_else(|| {
            let mut c: Vec<&(String, [f64; 3])> = factory
                .iter()
                .filter(|(n, _)| {
                    let nn = norm(n);
                    let toks: Vec<&str> = nn.split(' ').collect();
                    target.iter().all(|w| toks.contains(&w.as_str()))
                })
                .collect();
            c.sort_by_key(|(n, _)| (unwanted_finish(n, &asked_finish), n.len()));
            c.first().copied()
        })
        // 3. any paint of that colour, preferring plain finishes
        .or_else(|| {
            let want = want_colour?;
            let mut c: Vec<&(String, [f64; 3])> = factory.iter().filter(|(_, rgb)| classify(*rgb) == want).collect();
            c.sort_by_key(|(n, _)| (unwanted_finish(n, &asked_finish), n.len()));
            c.first().copied()
        })
        .cloned();

    let (paint_lua, paint_name) = match &pick {
        Some((n, _)) => (
            format!("core_vehicles.getModel({}).model.paints[{}]", lua_str(&veh.jbeam), lua_str(n)),
            n.clone(),
        ),
        None => {
            let Some(c) = want_colour else {
                return Ok(fail(label, format!("no paint called '{}' for the {}", tq, vname)));
            };
            let [r, g, b] = basic_rgb(c);
            let matte = asked_finish.contains(&"matte");
            (
                format!(
                    "{{baseColor={{{},{},{},1.2}},metallic={},roughness={},clearcoat={},clearcoatRoughness=0.05}}",
                    r,
                    g,
                    b,
                    if matte { 0.0 } else { 0.6 },
                    if matte { 0.9 } else { 0.5 },
                    if matte { 0 } else { 1 }
                ),
                format!("custom {}", c),
            )
        }
    };

    // Repaint the body (paint 1) and any other paint slot that matched the body colour.
    let mut slots = vec![1];
    if let Some(body) = veh.paints.first() {
        for (i, p) in veh.paints.iter().enumerate().skip(1) {
            if p == body {
                slots.push(i + 1);
            }
        }
    }
    let code = format!(
        "local m={} for _,i in ipairs({{{}}}) do core_vehicle_colors.setVehiclePaint(i, m, {}) end return 'ok'",
        paint_lua,
        slots.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","),
        veh.id
    );
    mcp.run_lua(&code)?;

    // Verify by reading the body colour back.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let now = vehicles(mcp)?
        .into_iter()
        .find(|v| v.id == veh.id)
        .and_then(|v| v.paints.first().copied());
    let ok = match (now, &pick) {
        (Some(c), Some((_, want))) => (c[0] - want[0]).abs() < 0.01 && (c[1] - want[1]).abs() < 0.01 && (c[2] - want[2]).abs() < 0.01,
        (Some(c), None) => want_colour.map(|w| classify(c) == w).unwrap_or(true),
        _ => false,
    };
    let label = format!("Paint {} {}", old, vname);
    let mut text = format!("now {}", paint_name);
    if more > 0 {
        text.push_str(&format!(" ({} other matching vehicle{} left as is)", more, if more == 1 { "" } else { "s" }));
    }
    Ok(Outcome { label, text: if ok { text } else { format!("{} (not confirmed)", text) }, status: if ok { "applied" } else { "partial" } })
}

fn unwanted_finish(name: &str, asked: &[&str]) -> bool {
    let n = name.to_lowercase();
    ["matte", "chrome"].iter().any(|f| n.contains(f) && !asked.contains(f))
}

#[cfg(test)]
mod tests {
    use super::classify;
    #[test]
    fn colours() {
        assert_eq!(classify([0.83, 0.83, 0.83]), "white");
        assert_eq!(classify([0.01, 0.01, 0.03]), "black");
        assert_eq!(classify([0.725, 0.063, 0.13]), "red");
        assert_eq!(classify([0.1, 0.4, 0.8]), "blue");
        assert_eq!(classify([0.23, 0.23, 0.23]), "grey");
        assert_eq!(classify([0.663, 0.594, 0.533]), "white"); // Almond Pearl
        assert_eq!(classify([0.35, 0.35, 0.36]), "grey");
    }
}
