//! The embedded Lua resolver and helpers to call it.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

use crate::mcp::{clip, Mcp};

pub const RESOLVER: &str = include_str!("../lua/resolver.lua");

/// Lua string literal (Lua 5.1 escapes; non-ASCII bytes pass through).
pub fn lua_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\{:03}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// JSON value -> Lua table/value literal.
pub fn lua_lit(v: &Value) -> String {
    match v {
        Value::Null => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => lua_str(s),
        Value::Array(a) => format!("{{{}}}", a.iter().map(lua_lit).collect::<Vec<_>>().join(",")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("[{}]={}", lua_str(k), lua_lit(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

/// Lua encodes empty tables as {} — treat both {} and [] as empty lists.
pub fn arr(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        _ => vec![],
    }
}

/// One resolver call (single vehicle-VM execution).
pub fn call(mcp: &Mcp, action: &str, args: &Value) -> Result<Value> {
    let tag = mcp.new_tag();
    let code = format!(
        "{}\nreturn BNGS.call({}, {}, nil, {})",
        RESOLVER,
        lua_str(action),
        lua_lit(args),
        lua_str(&tag)
    );
    let out = mcp.vehicle_lua(&code, &tag)?;
    serde_json::from_str(&out).with_context(|| format!("resolver returned bad JSON: {}", clip(&out, 200)))
}

/// Resolver call plus the deferred verify pass for electrics/gear/controller changes.
pub fn resolve(mcp: &Mcp, action: &str, args: &Value) -> Result<Value> {
    let mut r = call(mcp, action, args)?;
    if r["status"] == "pending" {
        if let Some(checks) = r.get("checks").cloned() {
            // Most changes show up within a frame or two; door latches and engine starts take longer.
            let wait = r["verify_ms"].as_u64().unwrap_or(400);
            std::thread::sleep(Duration::from_millis(wait));
            let mut pass = 1;
            let mut v = call(mcp, "verify", &json!({ "checks": checks, "action": action, "pass": pass }))?;
            // "pending" again: still in progress (an engine still cranking). Check again a few times.
            while v["status"] == "pending" && pass < 5 {
                pass += 1;
                std::thread::sleep(Duration::from_millis(1500));
                v = call(mcp, "verify", &json!({ "checks": checks, "action": action, "pass": pass }))?;
            }
            let from: Vec<Value> = arr(&r["targets"]).iter().map(|t| t["from"].clone()).collect();
            let mut targets: Vec<Value> = arr(&v["targets"]).into_iter().cloned().collect();
            for (i, t) in targets.iter_mut().enumerate() {
                if let Some(f) = from.get(i) {
                    t["from"] = f.clone();
                }
            }
            r["targets"] = Value::Array(targets);
            r["status"] = v["status"].clone();
        }
    }
    Ok(r)
}
