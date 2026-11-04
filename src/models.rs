//! Model files: where they live, downloading missing ones, and reporting their full paths.
//!
//! config.toml:
//!   [models]
//!   dir = "models"                 # folder for downloads; relative to the exe, or absolute
//!   [models.stt]
//!   file = "ggml-base.en.bin"      # file name inside `dir`
//!   path = ""                      # optional: full path to a model file elsewhere (overrides dir/file)
//!   url  = "https://..."           # download source when the file is missing
//!   sha256 = ""                    # optional checksum; empty skips verification

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::{ModelCfg, ModelsCfg};

/// Full path of a model: `path` if set, else `<dir>/<file>`. Relative paths are relative to the exe folder.
pub fn resolve(models: &ModelsCfg, m: &ModelCfg, exe_dir: &Path) -> PathBuf {
    let abs = |p: &Path| if p.is_absolute() { p.to_path_buf() } else { exe_dir.join(p) };
    if !m.path.trim().is_empty() {
        return abs(Path::new(m.path.trim()));
    }
    abs(Path::new(models.dir.trim())).join(&m.file)
}

/// Absolute, tidy path for display (no "\\?\" prefix on Windows).
pub fn display(p: &Path) -> String {
    let s = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).display().to_string();
    s.trim_start_matches(r"\\?\").to_string()
}

fn sha256_file(p: &Path) -> Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

fn human(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 { format!("{:.2} GB", mb / 1024.0) } else { format!("{:.1} MB", mb) }
}

/// Download `url` to `dest`, resuming a previous partial download (`<dest>.part`).
fn download(url: &str, dest: &Path) -> Result<()> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let part = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).build();
    let mut req = agent.get(url);
    if have > 0 {
        req = req.set("Range", &format!("bytes={}-", have));
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => bail!("download failed: HTTP {} from {}", code, url),
        Err(e) => bail!("download failed: {} ({})", e, url),
    };
    let resumed = resp.status() == 206;
    let start = if resumed { have } else { 0 };
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .map(|n| n + start);
    let mut out = if resumed {
        OpenOptions::new().append(true).open(&part)?
    } else {
        File::create(&part)?
    };
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = start;
    let t0 = Instant::now();
    let mut last = Instant::now() - Duration::from_secs(10);
    loop {
        let n = reader.read(&mut buf).context("reading download")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(500) {
            last = Instant::now();
            let rate = (done - start) as f64 / t0.elapsed().as_secs_f64().max(0.001);
            match total {
                Some(t) => print!("\r    {:>5.1}%  {} of {}  {}/s   ", done as f64 / t as f64 * 100.0, human(done), human(t), human(rate as u64)),
                None => print!("\r    {}  {}/s   ", human(done), human(rate as u64)),
            }
            std::io::stdout().flush().ok();
        }
    }
    out.flush()?;
    drop(out);
    println!("\r    100.0%  {}                              ", human(done));
    if let Some(t) = total {
        if done != t {
            bail!("download incomplete ({} of {}); run again to resume", human(done), human(t));
        }
    }
    fs::rename(&part, dest).with_context(|| format!("moving download to {}", dest.display()))?;
    Ok(())
}

/// Make sure every configured model exists (downloading if missing), print its full path,
/// and return (name, path) pairs.
pub fn ensure_all(models: &ModelsCfg, exe_dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let names: Vec<String> = models.entries.keys().cloned().collect();
    ensure_some(models, exe_dir, &names.iter().map(|s| s.as_str()).collect::<Vec<_>>())
}

/// Like ensure_all, but only for the named models (e.g. ["stt", "vad"]).
pub fn ensure_some(models: &ModelsCfg, exe_dir: &Path, wanted: &[&str]) -> Result<Vec<(String, PathBuf)>> {
    let mut names: Vec<&String> = models.entries.keys().filter(|k| wanted.contains(&k.as_str())).collect();
    names.sort();
    for w in wanted {
        if !models.entries.contains_key(*w) {
            bail!("config.toml has no [models.{}] section", w);
        }
    }
    let mut out = vec![];
    println!("Models (folder: {})", display(&resolve_dir(models, exe_dir)));
    for name in names {
        let m = &models.entries[name];
        let p = resolve(models, m, exe_dir);
        if !p.exists() {
            if m.url.trim().is_empty() {
                bail!("model '{}' not found at {} and no url is configured", name, p.display());
            }
            println!("  {}: downloading {}", name, m.url);
            println!("    to {}", p.display());
            download(&m.url, &p).with_context(|| format!("model '{}'", name))?;
        }
        if !m.sha256.trim().is_empty() {
            let got = sha256_file(&p)?;
            if !got.eq_ignore_ascii_case(m.sha256.trim()) {
                bail!("model '{}' checksum mismatch at {} (expected {}, got {}). Delete the file to re-download.", name, p.display(), m.sha256.trim(), got);
            }
        }
        let size = fs::metadata(&p).map(|x| x.len()).unwrap_or(0);
        println!("  {:<6} {}  ({})", name, display(&p), human(size));
        out.push((name.clone(), p));
    }
    Ok(out)
}

/// Just report where each model is (no downloads).
pub fn status(models: &ModelsCfg, exe_dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let mut names: Vec<&String> = models.entries.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let p = resolve(models, &models.entries[n], exe_dir);
            let ok = p.exists();
            (n.clone(), p, ok)
        })
        .collect()
}

pub fn resolve_dir(models: &ModelsCfg, exe_dir: &Path) -> PathBuf {
    let d = Path::new(models.dir.trim());
    if d.is_absolute() { d.to_path_buf() } else { exe_dir.join(d) }
}
