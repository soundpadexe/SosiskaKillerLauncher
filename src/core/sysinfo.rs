//! Physical memory size, so the RAM slider can go as high as the PC allows.

/// Total installed RAM in MiB (falls back to 16 GiB if it cannot be read).
pub fn total_ram_mb() -> u32 {
    static CACHE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| detect().filter(|&mb| mb >= 1024).unwrap_or(16384))
}

#[cfg(target_os = "linux")]
fn detect() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = s.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some((kb / 1024) as u32)
}

#[cfg(windows)]
fn detect() -> Option<u32> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
    }
    let mut m: MemoryStatusEx = unsafe { std::mem::zeroed() };
    m.length = std::mem::size_of::<MemoryStatusEx>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then(|| (m.total_phys / (1024 * 1024)) as u32)
}

#[cfg(target_os = "macos")]
fn detect() -> Option<u32> {
    let out = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output().ok()?;
    let bytes: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some((bytes / (1024 * 1024)) as u32)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn detect() -> Option<u32> {
    None
}

/// Slider ceiling: all installed RAM, rounded down to 512 MiB steps.
pub fn max_memory_mb() -> u32 {
    (total_ram_mb() / 512 * 512).max(1024)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_ram() {
        assert!(super::total_ram_mb() >= 1024);
    }
}
