//! Minimal MCP client for the BeamNG game server: JSON-RPC over streamable HTTP
//! (the game replies with plain JSON; SSE "data:" lines are handled too).

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::McpCfg;

pub struct Mcp {
    url: String,
    agent: ureq::Agent,
    id: AtomicU64,
    vehicle_timeout: Duration,
}

impl Mcp {
    pub fn new(cfg: &McpCfg) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(cfg.timeout_ms))
            .build();
        Mcp {
            url: format!("http://{}:{}{}", cfg.host, cfg.port, cfg.path),
            agent,
            id: AtomicU64::new(0),
            vehicle_timeout: Duration::from_millis(cfg.vehicle_timeout_ms),
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn post(&self, body: &Value) -> Result<String> {
        let resp = self
            .agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream")
            .send_string(&body.to_string());
        match resp {
            Ok(r) => Ok(r.into_string()?),
            Err(ureq::Error::Status(code, r)) => {
                bail!("MCP HTTP {}: {}", code, r.into_string().unwrap_or_default())
            }
            Err(e) => Err(anyhow!("cannot reach MCP at {}: {}", self.url, e)),
        }
    }

    fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.id.fetch_add(1, Ordering::SeqCst) + 1;
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let text = self.post(&body)?;
        let v = parse_body(&text)?;
        if let Some(err) = v.get("error") {
            bail!("MCP error: {}", err);
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    pub fn initialize(&self) -> Result<Value> {
        let r = self.rpc(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "BeamAssist", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        // Notification: no id, response body (if any) ignored.
        let _ = self.post(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        Ok(r)
    }

    pub fn list_tools(&self) -> Result<Vec<String>> {
        let r = self.rpc("tools/list", json!({}))?;
        Ok(r["tools"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t["name"].as_str().map(String::from)).collect())
            .unwrap_or_default())
    }

    /// Call a tool and return its text content. isError results become Err.
    pub fn call(&self, name: &str, args: Value) -> Result<String> {
        let r = self.rpc("tools/call", json!({ "name": name, "arguments": args }))?;
        let text = r["content"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if r["isError"].as_bool().unwrap_or(false) {
            bail!("{} failed: {}", name, text);
        }
        Ok(text)
    }

    /// Run Lua in the game-engine VM (synchronous).
    pub fn run_lua(&self, code: &str) -> Result<String> {
        self.call("run_lua", json!({ "code": code }))
    }

    /// Call a tool whose text result is JSON.
    pub fn call_json(&self, name: &str, args: Value) -> Result<Value> {
        let t = self.call(name, args)?;
        serde_json::from_str(&t).with_context(|| format!("{} returned non-JSON: {}", name, clip(&t, 200)))
    }

    pub fn new_tag(&self) -> String {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        format!("BNGS{}x{}", ms, self.id.fetch_add(1, Ordering::SeqCst))
    }

    /// Run Lua in the player vehicle VM. run_lua_vehicle is asynchronous: each call
    /// returns the PREVIOUS run's results, so we send the code once, then poll with a
    /// no-op until a result starting with `tag` appears. The code must return
    /// `tag .. <payload>`; the payload (without tag) is returned.
    pub fn vehicle_lua(&self, code: &str, tag: &str) -> Result<String> {
        self.call("run_lua_vehicle", json!({ "code": code }))?;
        let deadline = Instant::now() + self.vehicle_timeout;
        let mut polls = 0;
        loop {
            std::thread::sleep(Duration::from_millis(250));
            polls += 1;
            let out = self.call("run_lua_vehicle", json!({ "code": "return \"noop\"" }))?;
            let parsed: Option<Value> = serde_json::from_str(&out).ok();
            if let Some(Value::Object(map)) = &parsed {
                for v in map.values() {
                    if let Some(s) = v.as_str() {
                        if let Some(rest) = s.strip_prefix(tag) {
                            return Ok(rest.to_string());
                        }
                        // Anything other than our tag or the no-op after the first poll is
                        // our chunk's error message.
                        if polls >= 2 && s != "noop" && !s.is_empty() {
                            bail!("vehicle Lua error: {}", clip(s, 300));
                        }
                    }
                }
            }
            if Instant::now() > deadline {
                bail!("no response from vehicle");
            }
        }
    }

    pub fn show_message(&self, text: &str, icon: &str, ttl: f64, category: &str) -> Result<()> {
        self.call(
            "show_message",
            json!({ "text": text, "icon": icon, "ttl": ttl, "category": category }),
        )?;
        Ok(())
    }
}

fn parse_body(text: &str) -> Result<Value> {
    let t = text.trim();
    if t.starts_with('{') {
        return Ok(serde_json::from_str(t)?);
    }
    for line in t.lines() {
        if let Some(d) = line.strip_prefix("data:") {
            let d = d.trim();
            if d.starts_with('{') {
                return Ok(serde_json::from_str(d)?);
            }
        }
    }
    bail!("unexpected MCP response: {}", clip(t, 200))
}

pub fn clip(s: &str, n: usize) -> String {
    let c: String = s.chars().take(n).collect();
    if s.chars().count() > n { format!("{}…", c) } else { c }
}
