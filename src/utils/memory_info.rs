#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone)]
pub struct SystemMemoryInfo {
    pub total_mb: f64,
    pub available_mb: f64,
    pub used_mb: f64,
    pub usage_percent: f64,
}

pub fn get_system_memory_info() -> Result<SystemMemoryInfo, std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        get_linux_memory_info()
    }
    #[cfg(target_os = "macos")]
    {
        get_macos_memory_info()
    }
    #[cfg(target_os = "windows")]
    {
        get_windows_memory_info()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Ok(SystemMemoryInfo {
            total_mb: 0.0,
            available_mb: 0.0,
            used_mb: 0.0,
            usage_percent: 0.0,
        })
    }
}

#[cfg(target_os = "linux")]
fn get_linux_memory_info() -> Result<SystemMemoryInfo, std::io::Error> {
    let file = fs::File::open("/proc/meminfo")?;
    let reader = BufReader::new(file);

    let mut total_kb = 0;
    let mut available_kb = 0;
    let mut free_kb = 0;
    let mut buffers_kb = 0;
    let mut cached_kb = 0;

    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let value = parts[1].parse::<u64>().unwrap_or(0);
            match parts[0] {
                "MemTotal:" => total_kb = value,
                "MemAvailable:" => available_kb = value,
                "MemFree:" => free_kb = value,
                "Buffers:" => buffers_kb = value,
                "Cached:" => cached_kb = value,
                _ => {}
            }
        }
    }

    // If MemAvailable is not available, estimate it
    if available_kb == 0 {
        available_kb = free_kb + buffers_kb + cached_kb;
    }

    let total_mb = total_kb as f64 / 1024.0;
    let available_mb = available_kb as f64 / 1024.0;
    let used_mb = total_mb - available_mb;
    let usage_percent = (used_mb / total_mb) * 100.0;

    Ok(SystemMemoryInfo {
        total_mb,
        available_mb,
        used_mb,
        usage_percent,
    })
}

#[cfg(target_os = "macos")]
fn get_macos_memory_info() -> Result<SystemMemoryInfo, std::io::Error> {
    // On macOS, we'll use a simplified approach
    // In a real implementation, you'd use system calls or vm_stat
    Ok(SystemMemoryInfo {
        total_mb: 8192.0, // Placeholder
        available_mb: 4096.0,
        used_mb: 4096.0,
        usage_percent: 50.0,
    })
}

#[cfg(target_os = "windows")]
fn get_windows_memory_info() -> Result<SystemMemoryInfo, std::io::Error> {
    // On Windows, you'd use Windows API calls
    // For now, return placeholder values
    Ok(SystemMemoryInfo {
        total_mb: 8192.0, // Placeholder
        available_mb: 4096.0,
        used_mb: 4096.0,
        usage_percent: 50.0,
    })
}

pub fn get_process_memory_usage() -> Result<f64, std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        get_linux_process_memory()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(0.0) // Placeholder for other platforms
    }
}

#[cfg(target_os = "linux")]
fn get_linux_process_memory() -> Result<f64, std::io::Error> {
    let pid = std::process::id();
    let status_path = format!("/proc/{}/status", pid);

    let file = fs::File::open(status_path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        if line.starts_with("VmRSS:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb = parts[1].parse::<u64>().unwrap_or(0);
                return Ok(kb as f64 / 1024.0); // Convert to MB
            }
        }
    }

    Ok(0.0)
}

impl SystemMemoryInfo {
    pub fn print_summary(&self) {
        println!("💾 System Memory Info:");
        println!("   Total: {:.2} MB", self.total_mb);
        println!("   Available: {:.2} MB", self.available_mb);
        println!("   Used: {:.2} MB", self.used_mb);
        println!("   Usage: {:.1}%", self.usage_percent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_memory_info() {
        let info = get_system_memory_info();
        assert!(info.is_ok());

        let info = info.unwrap();
        assert!(info.total_mb >= 0.0);
        assert!(info.available_mb >= 0.0);
        assert!(info.used_mb >= 0.0);
        assert!(info.usage_percent >= 0.0 && info.usage_percent <= 100.0);
    }

    #[test]
    fn test_process_memory_usage() {
        let usage = get_process_memory_usage();
        assert!(usage.is_ok());

        let usage = usage.unwrap();
        assert!(usage >= 0.0);
    }
}
