//! Vehicles, configs and levels from the game, plus fuzzy matching of spoken names.

use anyhow::Result;
use serde_json::Value;

use crate::lua::{arr, lua_str};
use crate::mcp::Mcp;

#[derive(Debug, Clone)]
pub struct Model {
    pub key: String,
    pub brand: String,
    pub name: String,
    pub kind: String,
}

impl Model {
    pub fn label(&self) -> String {
        if self.brand.is_empty() { self.name.clone() } else { format!("{} {}", self.brand, self.name) }
    }
}

#[derive(Debug, Clone)]
pub struct Level {
    pub name: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct VConfig {
    pub key: String,
    pub name: String,
}

pub struct Catalog {
    pub models: Vec<Model>,
    pub levels: Vec<Level>,
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        o => o.to_string(),
    }
}

impl Catalog {
    pub fn load(mcp: &Mcp) -> Result<Self> {
        let m = mcp.run_lua(
            "local t={} for k,v in pairs(core_vehicles.getModelList().models) do \
             t[#t+1]={key=k,brand=v.Brand or '',name=v.Name or k,kind=v.Type or ''} end return jsonEncode(t)",
        )?;
        let mv: Value = serde_json::from_str(&m)?;
        let mut models: Vec<Model> = arr(&mv)
            .iter()
            .map(|x| Model { key: s(&x["key"]), brand: s(&x["brand"]), name: s(&x["name"]), kind: s(&x["kind"]) })
            .collect();
        models.sort_by(|a, b| a.label().cmp(&b.label()));

        let l = mcp.run_lua(
            "local t={} for _,l in ipairs(core_levels.getList()) do local tt=l.title \
             if translateLanguage then tt=translateLanguage(l.title,l.title,true) end \
             t[#t+1]={name=l.levelName,title=tostring(tt)} end return jsonEncode(t)",
        )?;
        let lv: Value = serde_json::from_str(&l)?;
        let levels = arr(&lv).iter().map(|x| Level { name: s(&x["name"]), title: s(&x["title"]) }).collect();
        Ok(Catalog { models, levels })
    }

    pub fn model(&self, key: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.key == key)
    }

    /// Configs of a model: (default config key, configs sorted by name).
    pub fn configs(&self, mcp: &Mcp, key: &str) -> Result<(String, Vec<VConfig>)> {
        let code = format!(
            "local m=core_vehicles.getModel({}) local t={{}} for k,c in pairs(m.configs or {{}}) do \
             t[#t+1]={{key=k,name=c.Configuration or k}} end \
             return jsonEncode({{default=m.model and m.model.default_pc or '',configs=t}})",
            lua_str(key)
        );
        let v: Value = serde_json::from_str(&mcp.run_lua(&code)?)?;
        let mut cfgs: Vec<VConfig> = arr(&v["configs"])
            .iter()
            .map(|c| VConfig { key: s(&c["key"]), name: s(&c["name"]) })
            .collect();
        cfgs.sort_by(|a, b| a.name.cmp(&b.name));
        let default = s(&v["default"]).trim_end_matches(".pc").to_string();
        Ok((default, cfgs))
    }

    /// Match the start of `words` to a model. Returns (model index, words used, score).
    pub fn match_model(&self, words: &[String]) -> Option<(usize, usize, f64)> {
        let mut best: Option<(usize, usize, f64)> = None;
        for n in 1..=words.len().min(4) {
            let q = words[..n].join(" ");
            for (i, m) in self.models.iter().enumerate() {
                let mut sc = lev(&q, &m.name).max(lev(&q, &m.label())).max(lev(&q, &m.key));
                if m.kind == "Prop" || m.kind == "Utility" {
                    sc -= 0.05;
                }
                let better = match best {
                    None => true,
                    Some((_, bn, bs)) => sc > bs + 1e-9 || ((sc - bs).abs() < 1e-9 && n > bn),
                };
                if better {
                    best = Some((i, n, sc));
                }
            }
        }
        best.filter(|b| b.2 >= 0.8)
    }

    pub fn match_level(&self, q: &str) -> Option<usize> {
        let c: Vec<(String, String)> = self.levels.iter().map(|l| (l.name.replace('_', " "), l.title.clone())).collect();
        best_match(q, &c, 0.55)
    }
}

pub fn match_config(q: &str, cfgs: &[VConfig]) -> Option<usize> {
    // Config names end in (M)/(A)/(DCT)...: map spoken transmission words onto them.
    let q: Vec<&str> = q
        .split_whitespace()
        .map(|w| match w {
            "manual" | "stick" => "m",
            "automatic" | "auto" => "a",
            other => other,
        })
        .collect();
    let q = q.join(" ");
    let c: Vec<(String, String)> = cfgs.iter().map(|x| (x.key.replace('_', " "), x.name.clone())).collect();
    best_match(&q, &c, 0.55)
}

// ---------------------------------------------------------------- fuzzy helpers

pub fn norm(s: &str) -> String {
    let mut o = String::new();
    for c in s.to_lowercase().chars() {
        o.push(if c.is_alphanumeric() { c } else { ' ' });
    }
    o.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collapse(s: &str) -> String {
    norm(s).replace(' ', "")
}

/// Normalised Levenshtein on space-less text ("le gran" == "LeGran", "d series" == "D-Series").
fn lev(a: &str, b: &str) -> f64 {
    let (a, b) = (collapse(a), collapse(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    strsim::normalized_levenshtein(&a, &b)
}

/// Fraction of query words found (fuzzily) in the candidate.
fn overlap(q: &str, c: &str) -> f64 {
    let qn = norm(q);
    let qt: Vec<&str> = qn.split(' ').filter(|x| !x.is_empty()).collect();
    if qt.is_empty() {
        return 0.0;
    }
    let cn = norm(c);
    let ct: Vec<&str> = cn.split(' ').collect();
    let hit = qt
        .iter()
        .filter(|w| ct.iter().any(|c| c == *w || (w.len() >= 4 && strsim::normalized_levenshtein(c, w) >= 0.8)))
        .count();
    hit as f64 / qt.len() as f64
}

fn phrase_score(q: &str, c: &str) -> f64 {
    overlap(q, c) * 0.7 + lev(q, c) * 0.3
}

/// cands = (key-ish, display name). Ties prefer the shorter display name.
fn best_match(q: &str, cands: &[(String, String)], min: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, (k, n)) in cands.iter().enumerate() {
        let sc = phrase_score(q, n).max(phrase_score(q, k));
        let better = match best {
            None => true,
            Some((bi, bs)) => sc > bs + 1e-9 || ((sc - bs).abs() < 1e-9 && n.len() < cands[bi].1.len()),
        };
        if better {
            best = Some((i, sc));
        }
    }
    best.filter(|b| b.1 >= min).map(|b| b.0)
}
