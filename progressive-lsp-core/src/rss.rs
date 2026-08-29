//! Process RSS sample. Not an allocator-matrix CI-arch winner.

#[cfg(target_os = "macos")]
use std::process::Command;

/// Current process RSS in bytes, if the host exposes it.
///
/// Darwin `ps -o rss=` is kilobytes. Linux `/proc/self/status` `VmRSS` is kB.
/// Record as a **Darwin sample** (or Linux sample) in docs — do not treat this
/// as a `xtask bench-alloc` cell winner ([testing.md](../../../docs/testing.md)).
pub fn sample_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        return linux_vm_rss_bytes();
    }
    #[cfg(target_os = "macos")]
    {
        return darwin_ps_rss_bytes();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_vm_rss_bytes() -> Option<u64> {
    parse_proc_status_vmrss(&std::fs::read_to_string("/proc/self/status").ok()?)
}

pub fn parse_proc_status_vmrss(status: &str) -> Option<u64> {
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("VmRSS:") else {
            continue;
        };
        let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
        return Some(kb.saturating_mul(1024));
    }
    None
}

#[cfg(target_os = "macos")]
fn darwin_ps_rss_bytes() -> Option<u64> {
    let pid = std::process::id().to_string();
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    rss_from_ps_output(out.status.success(), std::str::from_utf8(&out.stdout).ok()?)
}

pub fn rss_from_ps_output(status_ok: bool, stdout: &str) -> Option<u64> {
    if !status_ok {
        return None;
    }
    parse_ps_rss_kb(stdout)
}

pub fn parse_ps_rss_kb(text: &str) -> Option<u64> {
    let kb: u64 = text.split_whitespace().next()?.parse().ok()?;
    Some(kb.saturating_mul(1024))
}

/// Label for published numbers. Allocator winners stay CI-arch only.
pub fn rss_sample_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Darwin sample (not a CI-arch allocator-matrix winner)"
    } else if cfg!(target_os = "linux") {
        "Linux host sample (allocator winners only from matching CI arch)"
    } else {
        "host sample (allocator winners only from matching CI arch)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_and_ps_samples() {
        assert_eq!(
            parse_proc_status_vmrss("Name:\tfoo\nVmRSS:\t   123 kB\n"),
            Some(123 * 1024)
        );
        assert_eq!(parse_proc_status_vmrss("VmSize:\t1 kB\n"), None);
        assert_eq!(parse_proc_status_vmrss(""), None);
        assert_eq!(parse_proc_status_vmrss("VmRSS:\tnope"), None);
        assert_eq!(parse_ps_rss_kb("  42\n"), Some(42 * 1024));
        assert_eq!(parse_ps_rss_kb(""), None);
        assert_eq!(parse_ps_rss_kb("abc"), None);
        assert_eq!(rss_from_ps_output(false, "  42\n"), None);
        assert_eq!(rss_from_ps_output(true, "  42\n"), Some(42 * 1024));
        assert!(rss_sample_label().contains("allocator"));
        match sample_rss_bytes() {
            Some(n) => assert!(n > 1024, "rss bytes should be a real process sample, got {n}"),
            None => assert!(
                !cfg!(any(target_os = "macos", target_os = "linux")),
                "macos/linux must expose RSS"
            ),
        }
    }
}
