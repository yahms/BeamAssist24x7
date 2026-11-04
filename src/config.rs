//! config.toml beside the exe. Written with defaults on first run; user values are
//! merged over the defaults so a partial file still works.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub const DEFAULT_CONFIG: &str = r#"# BeamAssist configuration
# Created automatically on first run. Delete this file to regenerate the defaults.
# Any value left out falls back to its default.

[mcp]
# BeamNG game MCP server (or your TCP proxy to it)
host = "127.0.0.1"
port = 29292
path = "/mcp"
# HTTP timeout per MCP call (ms)
timeout_ms = 5000
# How long to wait for a vehicle Lua result (ms)
vehicle_timeout_ms = 4000

[messages]
# In-game message category: the same category replaces the previous message
category = "voice"
format = "{intent} — {result}"
ttl = 5.0
ttl_error = 10.0
ttl_list = 15.0
# Satnav, distances and other answers to questions
ttl_info = 15.0
# Show "Heard '...' — not understood" when a command isn't recognised
show_not_understood = true

[messages.result]
applied = "done"
already = "already {state}"
partial = "{ok} of {total} done"
unsupported = "not available on this vehicle"
failed = "failed {detail}"
no_response = "no response from vehicle"
not_understood = "not understood"

[messages.icons]
applied = "check_circle"
already = "info"
partial = "warning"
unsupported = "block"
failed = "error"

[router]
# Stage B: when a request isn't recognised word for word, compare it with example phrasings
# (bge-small embedding model, CPU) and run the closest command. false = rule parser only,
# which saves ~150 MB of RAM and skips downloading the embedding model.
enabled = true
# Minimum similarity (0-1) for a match. Higher = fewer wrong guesses, more "not understood".
threshold = 0.80
# Two different commands closer than this are treated as a tie: you're asked which you meant.
margin = 0.02
# Print the match and its score in the console
show_match = true

[voice]
# Voice input from the default microphone. false = text commands only.
enabled = true
# hotword: start with a wake phrase ("hey beam, high beams"), or say the wake phrase alone and
#          then the command within `listen_window` seconds. Safest; ignores chatter.
# both:    wake phrase optional for short (<= 8 words) commands the parser recognises.
# open:    no wake phrase; any recognised command runs (may react to talk/radio).
mode = "hotword"
wake_phrases = ["hey beam", "computer"]
listen_window = 6.0
# Where speech recognition runs:
#   auto = GPU (NVIDIA / AMD / Intel via Vulkan) if one has enough free VRAM, else CPU
#   gpu  = always try the GPU (falls back to CPU if it fails to start)
#   cpu  = always CPU
# GPU needs a build made with the Vulkan SDK installed (build.cmd does this automatically).
device = "auto"
# Which GPU when there are several (0 = first discrete GPU)
gpu_device = 0
# VRAM to leave free for the game (MB) when device = "auto"
vram_headroom_mb = 1024
# CPU threads for speech recognition
threads = 4
# Microphone level gate: speech must be above max(min_level, background noise * level_ratio)
min_level = 0.01
level_ratio = 3.0
# Silence that ends a sentence (ms), and the longest sentence (s)
end_silence_ms = 700
max_utterance_s = 10.0
# Silero VAD speech probability threshold (0..1) used to drop non-speech noise
vad_threshold = 0.5
# Print every transcript to the console
show_heard = true

# Speech/router models. Missing files are downloaded from `url` on first use
# (run `beam-assist.exe --models` to download and check them now).
[models]
# Folder for model files: relative to the exe, or an absolute path such as "D:/AI/models"
dir = "models"

# Each model: `file` is the name inside `dir`; set `path` to a full file path to use a
# model stored elsewhere (overrides dir/file). To use an alternative model, change file/path + url.
# sha256 = "" skips checksum verification.
[models.stt]
file = "ggml-base.en.bin"
path = ""
url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
sha256 = ""

[models.vad]
file = "ggml-silero-v5.1.2.bin"
path = ""
url = "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"
sha256 = ""

# Intent router embedding model (BERT, run with Candle): weights + config + tokenizer.
[models.embed]
file = "bge-small-en-v1.5.safetensors"
path = ""
url = "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/model.safetensors"
sha256 = ""

[models.embed_config]
file = "bge-small-en-v1.5.config.json"
path = ""
url = "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/config.json"
sha256 = ""

[models.embed_tokenizer]
file = "bge-small-en-v1.5.tokenizer.json"
path = ""
url = "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json"
sha256 = ""

# The LLM only picks between 2-3 candidate intents and fills slots (grammar-constrained JSON,
# a few tokens) - it never writes text, so a small quantised model is enough.
# Bigger alternative: Qwen/Qwen3-1.7B-GGUF  Qwen3-1.7B-Q8_0.gguf (1.8 GB)
[models.llm]
file = "Qwen_Qwen3-0.6B-Q4_K_M.gguf"
path = ""
url = "https://huggingface.co/bartowski/Qwen_Qwen3-0.6B-GGUF/resolve/main/Qwen_Qwen3-0.6B-Q4_K_M.gguf"
sha256 = ""

[models.llm_tokenizer]
file = "qwen3-0.6b.tokenizer.json"
path = ""
url = "https://huggingface.co/Qwen/Qwen3-0.6B/resolve/main/tokenizer.json"
sha256 = ""
"#;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub mcp: McpCfg,
    pub messages: MsgCfg,
    pub router: RouterCfg,
    pub voice: VoiceCfg,
    pub models: ModelsCfg,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelsCfg {
    pub dir: String,
    /// stt, vad, embed, llm, ... (every [models.<name>] table)
    #[serde(flatten)]
    pub entries: HashMap<String, ModelCfg>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpCfg {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub timeout_ms: u64,
    pub vehicle_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MsgCfg {
    pub category: String,
    pub format: String,
    pub ttl: f64,
    pub ttl_error: f64,
    pub ttl_list: f64,
    pub ttl_info: f64,
    pub show_not_understood: bool,
    pub result: HashMap<String, String>,
    pub icons: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouterCfg {
    pub enabled: bool,
    pub threshold: f32,
    pub margin: f32,
    pub show_match: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceCfg {
    pub enabled: bool,
    pub mode: String,
    pub wake_phrases: Vec<String>,
    pub listen_window: f64,
    pub threads: i32,
    pub device: String,
    pub gpu_device: i32,
    pub vram_headroom_mb: u64,
    pub min_level: f32,
    pub level_ratio: f32,
    pub end_silence_ms: u32,
    pub max_utterance_s: f32,
    pub vad_threshold: f32,
    pub show_heard: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelCfg {
    pub file: String,
    #[serde(default)]
    pub path: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        let both_tables = matches!(base.get(&k), Some(toml::Value::Table(_))) && v.is_table();
        if both_tables {
            if let (Some(toml::Value::Table(b)), toml::Value::Table(o)) = (base.get_mut(&k), v) {
                merge(b, o);
            }
        } else {
            base.insert(k, v);
        }
    }
}

/// Load config.toml, creating it with defaults if missing. Returns (config, created).
pub fn load_or_create(path: &Path) -> Result<(Config, bool)> {
    let created = if !path.exists() {
        std::fs::write(path, DEFAULT_CONFIG)
            .with_context(|| format!("writing default config to {}", path.display()))?;
        true
    } else {
        false
    };
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut base: toml::Table = toml::from_str(DEFAULT_CONFIG).expect("built-in default config is valid");
    let user: toml::Table = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    merge(&mut base, user);
    let cfg: Config = toml::Value::Table(base)
        .try_into()
        .with_context(|| format!("invalid values in {}", path.display()))?;
    Ok((cfg, created))
}
