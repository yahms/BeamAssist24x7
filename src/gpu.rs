//! GPU detection and device choice.
//! - Speech (whisper.cpp) uses the Vulkan backend when built with `--features gpu`:
//!   one build covers NVIDIA, AMD and Intel GPUs via the vulkan-1.dll every GPU driver installs.
//! - Free VRAM is read from DXGI (`IDXGIAdapter3::QueryVideoMemoryInfo`, windows 0.62): the OS
//!   budget for our process, which shrinks when the game and other apps use VRAM.

pub struct Adapter {
    pub name: String,
    pub vendor: &'static str,
    pub dedicated_mb: u64,
    pub budget_mb: u64,
    pub used_mb: u64,
}

impl Adapter {
    pub fn free_mb(&self) -> u64 {
        self.budget_mb.saturating_sub(self.used_mb)
    }
}

fn vendor(id: u32) -> &'static str {
    match id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        _ => "other",
    }
}

#[cfg(windows)]
pub fn adapters() -> Vec<Adapter> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::*;
    let mut out = vec![];
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return out };
        let mut i = 0u32;
        while let Ok(a) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(d) = a.GetDesc1() else { continue };
            // skip the software "Microsoft Basic Render Driver"
            if d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 || d.VendorId == 0x1414 {
                continue;
            }
            let name = String::from_utf16_lossy(&d.Description).trim_end_matches('\0').trim().to_string();
            let (mut budget, mut used) = (0u64, 0u64);
            if let Ok(a3) = a.cast::<IDXGIAdapter3>() {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if a3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).is_ok() {
                    budget = info.Budget;
                    used = info.CurrentUsage;
                }
            }
            out.push(Adapter {
                name,
                vendor: vendor(d.VendorId),
                dedicated_mb: d.DedicatedVideoMemory as u64 / 1_048_576,
                budget_mb: budget / 1_048_576,
                used_mb: used / 1_048_576,
            });
        }
    }
    out
}

#[cfg(not(windows))]
pub fn adapters() -> Vec<Adapter> {
    vec![]
}

pub enum Device {
    /// (ggml GPU device index, description)
    Gpu(i32, String),
    /// reason
    Cpu(String),
}

/// Decide where a model runs.
/// mode: "auto" (GPU if one has enough free VRAM), "gpu" (always try GPU), "cpu".
/// need_mb: rough VRAM the model needs; headroom_mb: VRAM to leave free for the game.
pub fn choose(mode: &str, gpu_device: i32, need_mb: u64, headroom_mb: u64, built_with_gpu: bool) -> Device {
    let mode = mode.trim().to_lowercase();
    if mode == "cpu" {
        return Device::Cpu("device = \"cpu\" in config".into());
    }
    if !built_with_gpu {
        return Device::Cpu("this build has no GPU support (install the Vulkan SDK and rebuild)".into());
    }
    let list = adapters();
    for a in &list {
        println!(
            "  GPU: {} ({}), {} MB dedicated, {} MB free of {} MB budget",
            a.name,
            a.vendor,
            a.dedicated_mb,
            a.free_mb(),
            a.budget_mb
        );
    }
    // ggml-vulkan device 0 is normally the first discrete GPU; describe the biggest one.
    let best = list.iter().max_by_key(|a| a.dedicated_mb);
    if mode == "gpu" {
        let what = best.map(|a| a.name.clone()).unwrap_or_else(|| "GPU".into());
        return Device::Gpu(gpu_device, format!("{} (forced in config)", what));
    }
    match best {
        None => Device::Cpu("no GPU found".into()),
        Some(a) if a.budget_mb == 0 => Device::Gpu(gpu_device, format!("{} (VRAM unknown)", a.name)),
        Some(a) if a.free_mb() >= need_mb + headroom_mb => {
            Device::Gpu(gpu_device, format!("{}, {} MB free", a.name, a.free_mb()))
        }
        Some(a) => Device::Cpu(format!(
            "{} has {} MB free VRAM; need {} MB + {} MB headroom for the game",
            a.name,
            a.free_mb(),
            need_mb,
            headroom_mb
        )),
    }
}
