//! Fail if a shipped ELF has a dynamic interpreter or any `DT_NEEDED`.
//! Refuses to pass Mach-O (do not fake green on Darwin host binaries).
//!
//! [`StaticCheckPolicy::NativeImageLibc`] is the aarch64 `javacs` exception:
//! libc `DT_NEEDED` / PT_INTERP may pass; `libjvm`, JAR, and Mach-O still fail.
//! [`check_path`] / `xtask check-static` stay the fully-static bar.

use std::fs;
use std::path::Path;

const PT_INTERP: u32 = 3;
const PT_DYNAMIC: u32 = 2;
const DT_NEEDED: i64 = 1;
const DT_STRTAB: i64 = 5;
const DT_NULL: i64 = 0;
const PT_LOAD: u32 = 1;
const ELF_MAGIC: &[u8] = b"\x7fELF";
const MACHO_32: [u8; 4] = [0xfe, 0xed, 0xfa, 0xce];
const MACHO_64: [u8; 4] = [0xfe, 0xed, 0xfa, 0xcf];
const MACHO_32_SW: [u8; 4] = [0xce, 0xfa, 0xed, 0xfe];
const MACHO_64_SW: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
const FAT_MAGIC: [u8; 4] = [0xca, 0xfe, 0xba, 0xbe];
const JAR_LOCAL: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const JAR_EMPTY: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const JAR_SPANNED: [u8; 4] = [0x50, 0x4b, 0x07, 0x08];

/// How strictly we inspect a dest ELF after extract.
/// Value object: [`FullyStatic`](Self::FullyStatic) is `xtask check-static`;
/// [`NativeImageLibc`](Self::NativeImageLibc) is the aarch64 javacs exception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticCheckPolicy {
    /// No PT_INTERP, no `DT_NEEDED`. Default for every pack except aarch64 javacs.
    FullyStatic,
    /// aarch64 native-image javacs: libc `DT_NEEDED` / PT_INTERP allowed;
    /// `libjvm` / JAR / Mach-O fail closed.
    NativeImageLibc,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StaticLinkError {
    #[error("not an ELF (Mach-O); refusing to pass a Darwin binary")]
    MachO,
    #[error("not an ELF (JAR); refusing a Java archive as the engine")]
    Jar,
    #[error("not an ELF (magic mismatch)")]
    NotElf,
    #[error("truncated ELF")]
    Truncated,
    #[error("dynamic interpreter (PT_INTERP) present")]
    DynamicInterpreter,
    #[error("DT_NEEDED present: {0}")]
    DtNeeded(String),
    #[error("io: {0}")]
    Io(String),
}

pub fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("check-static requires at least one ELF path".into());
    }
    for path in args {
        check_path(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

pub fn check_path(path: &Path) -> Result<(), StaticLinkError> {
    check_path_with_policy(path, StaticCheckPolicy::FullyStatic)
}

/// aarch64 `javacs` dest: libc `DT_NEEDED` / PT_INTERP allowed; `libjvm` / JAR / Mach-O fail.
pub fn check_native_image_libc(path: &Path) -> Result<(), StaticLinkError> {
    check_path_with_policy(path, StaticCheckPolicy::NativeImageLibc)
}

pub fn check_path_with_policy(
    path: &Path,
    policy: StaticCheckPolicy,
) -> Result<(), StaticLinkError> {
    let data = fs::read(path).map_err(|e| StaticLinkError::Io(e.to_string()))?;
    check_bytes_with_policy(&data, policy)
}

#[allow(dead_code)]
pub fn check_bytes(data: &[u8]) -> Result<(), StaticLinkError> {
    check_bytes_with_policy(data, StaticCheckPolicy::FullyStatic)
}

pub fn check_bytes_with_policy(
    data: &[u8],
    policy: StaticCheckPolicy,
) -> Result<(), StaticLinkError> {
    if data.len() >= 4 {
        let magic = [data[0], data[1], data[2], data[3]];
        if magic == MACHO_32
            || magic == MACHO_64
            || magic == MACHO_32_SW
            || magic == MACHO_64_SW
            || magic == FAT_MAGIC
        {
            return Err(StaticLinkError::MachO);
        }
        if magic == JAR_LOCAL || magic == JAR_EMPTY || magic == JAR_SPANNED {
            return Err(StaticLinkError::Jar);
        }
    }
    if data.len() < 16 || &data[..4] != ELF_MAGIC {
        return Err(StaticLinkError::NotElf);
    }
    let class = data[4];
    let endian = data[5];
    match (class, endian) {
        (1, 1) => check_elf32(data, false, policy),
        (1, 2) => check_elf32(data, true, policy),
        (2, 1) => check_elf64(data, false, policy),
        (2, 2) => check_elf64(data, true, policy),
        _ => Err(StaticLinkError::NotElf),
    }
}

fn check_elf64(
    data: &[u8],
    be: bool,
    policy: StaticCheckPolicy,
) -> Result<(), StaticLinkError> {
    if data.len() < 64 {
        return Err(StaticLinkError::Truncated);
    }
    let phoff = u64_at(data, 32, be)? as usize;
    let phentsize = u16_at(data, 54, be)? as usize;
    let phnum = u16_at(data, 56, be)? as usize;
    walk_program_headers(data, be, phoff, phentsize, phnum, true, policy)
}

fn check_elf32(
    data: &[u8],
    be: bool,
    policy: StaticCheckPolicy,
) -> Result<(), StaticLinkError> {
    if data.len() < 52 {
        return Err(StaticLinkError::Truncated);
    }
    let phoff = u32_at(data, 28, be)? as usize;
    let phentsize = u16_at(data, 42, be)? as usize;
    let phnum = u16_at(data, 44, be)? as usize;
    walk_program_headers(data, be, phoff, phentsize, phnum, false, policy)
}

fn walk_program_headers(
    data: &[u8],
    be: bool,
    phoff: usize,
    phentsize: usize,
    phnum: usize,
    is64: bool,
    policy: StaticCheckPolicy,
) -> Result<(), StaticLinkError> {
    if phentsize == 0 {
        return Err(StaticLinkError::Truncated);
    }
    for i in 0..phnum {
        let off = phoff.saturating_add(i.saturating_mul(phentsize));
        if off.saturating_add(phentsize) > data.len() {
            return Err(StaticLinkError::Truncated);
        }
        let p_type = u32_at(data, off, be)?;
        if p_type == PT_INTERP {
            match policy {
                StaticCheckPolicy::FullyStatic => {
                    return Err(StaticLinkError::DynamicInterpreter);
                }
                StaticCheckPolicy::NativeImageLibc => {
                    if !interp_is_libc(data, be, off, is64) {
                        return Err(StaticLinkError::DynamicInterpreter);
                    }
                }
            }
        }
        if p_type == PT_DYNAMIC {
            check_dynamic(data, be, off, is64, policy)?;
        }
    }
    Ok(())
}

fn interp_is_libc(data: &[u8], be: bool, ph_off: usize, is64: bool) -> bool {
    let Some(path) = interp_path(data, be, ph_off, is64) else {
        return false;
    };
    is_libc_interp(&path)
}

fn interp_path(data: &[u8], be: bool, ph_off: usize, is64: bool) -> Option<String> {
    let (offset, filesz) = if is64 {
        (
            u64_at(data, ph_off + 8, be).ok()? as usize,
            u64_at(data, ph_off + 32, be).ok()? as usize,
        )
    } else {
        (
            u32_at(data, ph_off + 4, be).ok()? as usize,
            u32_at(data, ph_off + 16, be).ok()? as usize,
        )
    };
    if filesz == 0 || offset.saturating_add(filesz) > data.len() {
        return None;
    }
    let slice = &data[offset..offset + filesz];
    let n = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    if n == 0 {
        return None;
    }
    String::from_utf8(slice[..n].to_vec()).ok()
}

fn check_dynamic(
    data: &[u8],
    be: bool,
    ph_off: usize,
    is64: bool,
    policy: StaticCheckPolicy,
) -> Result<(), StaticLinkError> {
    let (offset, filesz) = if is64 {
        (
            u64_at(data, ph_off + 8, be)? as usize,
            u64_at(data, ph_off + 32, be)? as usize,
        )
    } else {
        (
            u32_at(data, ph_off + 4, be)? as usize,
            u32_at(data, ph_off + 16, be)? as usize,
        )
    };
    let entry = if is64 { 16 } else { 8 };
    if entry == 0 || offset.saturating_add(filesz) > data.len() {
        return Err(StaticLinkError::Truncated);
    }
    let mut pos = offset;
    let end = offset + filesz;
    let mut strtab_vaddr: Option<u64> = None;
    let mut needed_vals: Vec<u64> = Vec::new();
    while pos + entry <= end {
        let tag = if is64 {
            i64_at(data, pos, be)?
        } else {
            i32_at(data, pos, be)? as i64
        };
        if tag == DT_NULL {
            break;
        }
        let val = if is64 {
            u64_at(data, pos + 8, be)?
        } else {
            u32_at(data, pos + 4, be)? as u64
        };
        if tag == DT_STRTAB {
            strtab_vaddr = Some(val);
        } else if tag == DT_NEEDED {
            needed_vals.push(val);
        }
        pos += entry;
    }
    let strtab_file = strtab_vaddr
        .and_then(|vaddr| vaddr_to_file_offset(data, be, is64, vaddr));
    for val in needed_vals {
        let name = needed_soname(data, strtab_file, val as usize)
            .unwrap_or_else(|| "DT_NEEDED".into());
        if is_libjvm(&name) {
            return Err(StaticLinkError::DtNeeded("libjvm".into()));
        }
        if is_libdl(&name) {
            return Err(StaticLinkError::DtNeeded("libdl".into()));
        }
        if policy == StaticCheckPolicy::NativeImageLibc && is_libc_soname(&name) {
            continue;
        }
        return Err(StaticLinkError::DtNeeded(name));
    }
    Ok(())
}

fn vaddr_to_file_offset(data: &[u8], be: bool, is64: bool, vaddr: u64) -> Option<usize> {
    let (phoff, phentsize, phnum) = if is64 {
        (
            u64_at(data, 32, be).ok()? as usize,
            u16_at(data, 54, be).ok()? as usize,
            u16_at(data, 56, be).ok()? as usize,
        )
    } else {
        (
            u32_at(data, 28, be).ok()? as usize,
            u16_at(data, 42, be).ok()? as usize,
            u16_at(data, 44, be).ok()? as usize,
        )
    };
    if phentsize == 0 {
        return None;
    }
    for i in 0..phnum {
        let off = phoff.saturating_add(i.saturating_mul(phentsize));
        if off.saturating_add(phentsize) > data.len() {
            break;
        }
        if u32_at(data, off, be).ok()? != PT_LOAD {
            continue;
        }
        let (p_offset, p_vaddr, p_filesz) = if is64 {
            (
                u64_at(data, off + 8, be).ok()?,
                u64_at(data, off + 16, be).ok()?,
                u64_at(data, off + 32, be).ok()?,
            )
        } else {
            (
                u32_at(data, off + 4, be).ok()? as u64,
                u32_at(data, off + 8, be).ok()? as u64,
                u32_at(data, off + 16, be).ok()? as u64,
            )
        };
        if vaddr >= p_vaddr && vaddr < p_vaddr.saturating_add(p_filesz) {
            return Some((p_offset + (vaddr - p_vaddr)) as usize);
        }
    }
    None
}

/// Resolve a `DT_NEEDED` string: offset into `DT_STRTAB` when mapped, else direct file offset (fixtures).
fn needed_soname(data: &[u8], strtab_file: Option<usize>, val: usize) -> Option<String> {
    let file_off = strtab_file.map(|base| base.saturating_add(val)).unwrap_or(val);
    if file_off >= data.len() {
        return None;
    }
    let n = data[file_off..].iter().position(|&b| b == 0)?;
    if n == 0 {
        return None;
    }
    String::from_utf8(data[file_off..file_off + n].to_vec()).ok()
}

/// Fail closed: sqlite loadable extensions pull `libdl` (`DT_NEEDED`).
fn is_libdl(name: &str) -> bool {
    name == "libdl" || name.starts_with("libdl.so") || name.contains("libdl.so")
}

fn is_libjvm(name: &str) -> bool {
    name == "libjvm" || name.starts_with("libjvm.so") || name.contains("libjvm.so")
}

/// Host C library only — not libm/libpthread/libdl (those stay fail-closed).
fn is_libc_soname(name: &str) -> bool {
    name == "libc"
        || name.starts_with("libc.so")
        || name.contains("libc.so")
        || is_libc_interp(name)
}

fn is_libc_interp(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.starts_with("ld-linux")
        || base.starts_with("ld-linux-")
        || base == "ld.so.1"
        || base.starts_with("ld.so.")
        || base.starts_with("ld-linux-aarch64")
        || base.starts_with("ld-linux-x86-64")
}

fn u16_at(data: &[u8], off: usize, be: bool) -> Result<u16, StaticLinkError> {
    let b = data.get(off..off + 2).ok_or(StaticLinkError::Truncated)?;
    Ok(if be {
        u16::from_be_bytes([b[0], b[1]])
    } else {
        u16::from_le_bytes([b[0], b[1]])
    })
}

fn u32_at(data: &[u8], off: usize, be: bool) -> Result<u32, StaticLinkError> {
    let b = data.get(off..off + 4).ok_or(StaticLinkError::Truncated)?;
    Ok(if be {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    })
}

fn u64_at(data: &[u8], off: usize, be: bool) -> Result<u64, StaticLinkError> {
    let b = data.get(off..off + 8).ok_or(StaticLinkError::Truncated)?;
    Ok(if be {
        u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    } else {
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    })
}

fn i32_at(data: &[u8], off: usize, be: bool) -> Result<i32, StaticLinkError> {
    Ok(u32_at(data, off, be)? as i32)
}

fn i64_at(data: &[u8], off: usize, be: bool) -> Result<i64, StaticLinkError> {
    Ok(u64_at(data, off, be)? as i64)
}

/// Minimal ELF64 LE: one PT_LOAD, no PT_INTERP, no PT_DYNAMIC.
/// [`crate::musl::RecordingDockerPort`] writes this so dest exists without docker.
#[cfg(test)]
pub fn fixture_static_elf64() -> Vec<u8> {
    let mut e = vec![0u8; 128];
    e[0..4].copy_from_slice(ELF_MAGIC);
    e[4] = 2;
    e[5] = 1;
    e[6] = 1;
    e[16] = 2;
    e[18] = 62;
    e[20] = 1;
    e[32] = 64;
    e[52] = 64;
    e[54] = 56;
    e[56] = 1;
    e[64] = 1;
    e
}

/// ELF64 LE with PT_INTERP and PT_DYNAMIC/DT_NEEDED.
#[cfg(test)]
pub fn fixture_dynamic_elf64() -> Vec<u8> {
    let mut e = vec![0u8; 256];
    e[0..4].copy_from_slice(ELF_MAGIC);
    e[4] = 2;
    e[5] = 1;
    e[6] = 1;
    e[16] = 3;
    e[18] = 62;
    e[20] = 1;
    e[32] = 64;
    e[52] = 64;
    e[54] = 56;
    e[56] = 2;
    // ph[0] PT_INTERP
    e[64] = 3;
    // ph[1] PT_DYNAMIC at offset 64+56=120
    let ph1 = 64 + 56;
    e[ph1..ph1 + 4].copy_from_slice(&2u32.to_le_bytes());
    e[ph1 + 8..ph1 + 16].copy_from_slice(&200u64.to_le_bytes());
    e[ph1 + 32..ph1 + 40].copy_from_slice(&32u64.to_le_bytes());
    // dynamic entries at 200: DT_NEEDED, DT_NULL
    e[200..208].copy_from_slice(&1u64.to_le_bytes());
    e
}

/// ELF64 LE with PT_DYNAMIC / DT_NEEDED = `libc.so.6` (no PT_INTERP).
#[cfg(test)]
pub fn fixture_libc_needed_elf64() -> Vec<u8> {
    named_needed_elf64(b"libc.so.6")
}

/// ELF64 LE with PT_INTERP = `/lib/ld-linux-aarch64.so.1` (no PT_DYNAMIC).
#[cfg(test)]
pub fn fixture_libc_interp_elf64() -> Vec<u8> {
    let mut e = vec![0u8; 320];
    e[0..4].copy_from_slice(ELF_MAGIC);
    e[4] = 2;
    e[5] = 1;
    e[6] = 1;
    e[16] = 3;
    e[18] = 62;
    e[20] = 1;
    e[32] = 64;
    e[52] = 64;
    e[54] = 56;
    e[56] = 1;
    e[64] = 3;
    let interp = b"/lib/ld-linux-aarch64.so.1\0";
    e[64 + 8..64 + 16].copy_from_slice(&200u64.to_le_bytes());
    e[64 + 32..64 + 40].copy_from_slice(&(interp.len() as u64).to_le_bytes());
    e[200..200 + interp.len()].copy_from_slice(interp);
    e
}

/// ELF64 LE with PT_DYNAMIC / DT_NEEDED = `libjvm.so` (no PT_INTERP).
#[cfg(test)]
pub fn fixture_libjvm_elf64() -> Vec<u8> {
    named_needed_elf64(b"libjvm.so")
}

#[cfg(test)]
pub fn fixture_jar() -> Vec<u8> {
    let mut e = vec![0u8; 32];
    e[0..4].copy_from_slice(&JAR_LOCAL);
    e
}

#[cfg(test)]
fn named_needed_elf64(name: &[u8]) -> Vec<u8> {
    let mut e = vec![0u8; 320];
    e[0..4].copy_from_slice(ELF_MAGIC);
    e[4] = 2;
    e[5] = 1;
    e[6] = 1;
    e[16] = 2;
    e[18] = 62;
    e[20] = 1;
    e[32] = 64;
    e[52] = 64;
    e[54] = 56;
    e[56] = 1;
    e[64] = 2;
    e[64 + 8..64 + 16].copy_from_slice(&200u64.to_le_bytes());
    e[64 + 32..64 + 40].copy_from_slice(&16u64.to_le_bytes());
    e[200..208].copy_from_slice(&1i64.to_le_bytes());
    e[208..216].copy_from_slice(&216u64.to_le_bytes());
    e[216..216 + name.len()].copy_from_slice(name);
    e
}

/// ELF64 LE with PT_DYNAMIC / DT_NEEDED = `libdl.so.2` (no PT_INTERP).
#[cfg(test)]
pub fn fixture_libdl_elf64() -> Vec<u8> {
    named_needed_elf64(b"libdl.so.2")
}

#[cfg(test)]
pub fn fixture_macho() -> Vec<u8> {
    let mut e = vec![0u8; 32];
    e[0..4].copy_from_slice(&MACHO_64_SW);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_fixture_passes() {
        check_bytes(&fixture_static_elf64()).unwrap();
    }

    #[test]
    fn dynamic_fixture_fails_interp_or_needed() {
        let err = check_bytes(&fixture_dynamic_elf64()).unwrap_err();
        assert!(
            matches!(
                err,
                StaticLinkError::DynamicInterpreter | StaticLinkError::DtNeeded(_)
            ),
            "{err:?}"
        );
    }

    #[test]
    fn macho_is_refused() {
        assert_eq!(check_bytes(&fixture_macho()), Err(StaticLinkError::MachO));
        let mut fat = vec![0u8; 8];
        fat[0..4].copy_from_slice(&FAT_MAGIC);
        assert_eq!(check_bytes(&fat), Err(StaticLinkError::MachO));
        assert_eq!(
            check_bytes(&[0xfe, 0xed, 0xfa, 0xce, 0, 0, 0, 0]),
            Err(StaticLinkError::MachO)
        );
        assert_eq!(
            check_bytes(&[0xfe, 0xed, 0xfa, 0xcf, 0, 0, 0, 0]),
            Err(StaticLinkError::MachO)
        );
        assert_eq!(
            check_bytes(&[0xce, 0xfa, 0xed, 0xfe, 0, 0, 0, 0]),
            Err(StaticLinkError::MachO)
        );
    }

    #[test]
    fn not_elf_and_truncated() {
        assert_eq!(check_bytes(b"nope"), Err(StaticLinkError::NotElf));
        let mut short = fixture_static_elf64();
        short.truncate(20);
        assert_eq!(check_bytes(&short), Err(StaticLinkError::Truncated));
        let mut bad_class = fixture_static_elf64();
        bad_class[4] = 9;
        assert_eq!(check_bytes(&bad_class), Err(StaticLinkError::NotElf));
    }

    #[test]
    fn interp_alone_fails() {
        let mut e = fixture_static_elf64();
        e[64] = 3;
        assert_eq!(check_bytes(&e), Err(StaticLinkError::DynamicInterpreter));
    }

    #[test]
    fn libdl_dt_needed_fails_closed() {
        let err = check_bytes(&fixture_libdl_elf64()).unwrap_err();
        assert_eq!(err, StaticLinkError::DtNeeded("libdl".into()));
        assert!(err.to_string().contains("libdl"));
        assert!(is_libdl("libdl"));
        assert!(is_libdl("libdl.so.2"));
        assert!(!is_libdl("libc.so.6"));
        assert!(!is_libdl("DT_NEEDED"));
    }

    #[test]
    fn dt_needed_without_interp_fails() {
        let mut e = fixture_static_elf64();
        e[56] = 1;
        e[64] = 2;
        e[64 + 8..64 + 16].copy_from_slice(&200u64.to_le_bytes());
        e[64 + 32..64 + 40].copy_from_slice(&16u64.to_le_bytes());
        e.resize(256, 0);
        e[200..208].copy_from_slice(&1i64.to_le_bytes());
        assert!(matches!(check_bytes(&e), Err(StaticLinkError::DtNeeded(_))));
    }

    #[test]
    fn check_path_and_run() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("static.elf");
        fs::write(&p, fixture_static_elf64()).unwrap();
        check_path(&p).unwrap();
        run(&[p.display().to_string()]).unwrap();
        assert!(run(&[]).is_err());
        let d = dir.path().join("dyn.elf");
        fs::write(&d, fixture_dynamic_elf64()).unwrap();
        assert!(run(&[d.display().to_string()]).is_err());
        assert!(check_path(Path::new("/no-such-elf-plsp")).is_err());
    }

    #[test]
    fn elf32_static_and_be_paths() {
        let mut e = vec![0u8; 128];
        e[0..4].copy_from_slice(ELF_MAGIC);
        e[4] = 1;
        e[5] = 1;
        e[6] = 1;
        e[28] = 52;
        e[42] = 32;
        e[44] = 1;
        e[52] = 1;
        check_bytes(&e).unwrap();

        let mut be = vec![0u8; 128];
        be[0..4].copy_from_slice(ELF_MAGIC);
        be[4] = 2;
        be[5] = 2;
        be[6] = 1;
        be[32..40].copy_from_slice(&64u64.to_be_bytes());
        be[52..54].copy_from_slice(&64u16.to_be_bytes());
        be[54..56].copy_from_slice(&56u16.to_be_bytes());
        be[56..58].copy_from_slice(&1u16.to_be_bytes());
        be[64..68].copy_from_slice(&1u32.to_be_bytes());
        check_bytes(&be).unwrap();

        let mut e32be = vec![0u8; 128];
        e32be[0..4].copy_from_slice(ELF_MAGIC);
        e32be[4] = 1;
        e32be[5] = 2;
        e32be[28..32].copy_from_slice(&52u32.to_be_bytes());
        e32be[42..44].copy_from_slice(&32u16.to_be_bytes());
        e32be[44..46].copy_from_slice(&1u16.to_be_bytes());
        e32be[52..56].copy_from_slice(&1u32.to_be_bytes());
        check_bytes(&e32be).unwrap();
    }

    #[test]
    fn error_displays() {
        assert!(StaticLinkError::MachO.to_string().contains("Mach-O"));
        assert!(StaticLinkError::Jar.to_string().contains("JAR"));
        assert!(StaticLinkError::DynamicInterpreter
            .to_string()
            .contains("PT_INTERP"));
        assert!(StaticLinkError::DtNeeded("libc".into())
            .to_string()
            .contains("libc"));
        assert!(StaticLinkError::Io("x".into()).to_string().contains("x"));
        assert!(StaticLinkError::NotElf.to_string().contains("ELF"));
        assert!(StaticLinkError::Truncated.to_string().contains("truncated"));
    }

    #[test]
    fn libc_needed_passes_native_image_libc_and_fails_check_path() {
        let elf = fixture_libc_needed_elf64();
        assert!(matches!(
            check_bytes(&elf),
            Err(StaticLinkError::DtNeeded(n)) if n.contains("libc")
        ));
        check_bytes_with_policy(&elf, StaticCheckPolicy::NativeImageLibc).unwrap();
        assert_eq!(
            StaticCheckPolicy::FullyStatic,
            StaticCheckPolicy::FullyStatic
        );
        assert_ne!(
            StaticCheckPolicy::FullyStatic,
            StaticCheckPolicy::NativeImageLibc
        );
    }

    #[test]
    fn libc_interp_passes_native_image_libc_and_fails_check_path() {
        let elf = fixture_libc_interp_elf64();
        assert_eq!(check_bytes(&elf), Err(StaticLinkError::DynamicInterpreter));
        check_bytes_with_policy(&elf, StaticCheckPolicy::NativeImageLibc).unwrap();
        assert!(is_libc_interp("/lib/ld-linux-aarch64.so.1"));
        assert!(is_libc_interp("/lib64/ld-linux-x86-64.so.2"));
        assert!(!is_libc_interp("/lib/ld-musl-aarch64.so.1"));
    }

    #[test]
    fn libjvm_fails_native_image_libc() {
        let elf = fixture_libjvm_elf64();
        let err = check_bytes_with_policy(&elf, StaticCheckPolicy::NativeImageLibc).unwrap_err();
        assert_eq!(err, StaticLinkError::DtNeeded("libjvm".into()));
        assert!(check_bytes(&elf).is_err());
        assert!(is_libjvm("libjvm.so"));
        assert!(!is_libjvm("libc.so.6"));
    }

    #[test]
    fn jar_and_macho_fail_native_image_libc() {
        assert_eq!(
            check_bytes_with_policy(&fixture_jar(), StaticCheckPolicy::NativeImageLibc),
            Err(StaticLinkError::Jar)
        );
        assert_eq!(check_bytes(&fixture_jar()), Err(StaticLinkError::Jar));
        assert_eq!(
            check_bytes_with_policy(&fixture_macho(), StaticCheckPolicy::NativeImageLibc),
            Err(StaticLinkError::MachO)
        );
        assert_eq!(
            check_bytes_with_policy(b"nope", StaticCheckPolicy::NativeImageLibc),
            Err(StaticLinkError::NotElf)
        );
    }

    #[test]
    fn live_aarch64_javacs_passes_native_image_libc_when_built() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../target/musl/aarch64-unknown-linux-musl/engines/java/javacs",
        );
        if !path.is_file() {
            return;
        }
        check_native_image_libc(&path).unwrap();
        assert!(check_path(&path).is_err());
    }

    #[test]
    fn check_native_image_libc_path_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let libc = dir.path().join("libc.elf");
        fs::write(&libc, fixture_libc_needed_elf64()).unwrap();
        check_native_image_libc(&libc).unwrap();
        assert!(check_path(&libc).is_err());
        let jvm = dir.path().join("jvm.elf");
        fs::write(&jvm, fixture_libjvm_elf64()).unwrap();
        assert!(check_native_image_libc(&jvm).is_err());
        let jar = dir.path().join("engine.jar");
        fs::write(&jar, fixture_jar()).unwrap();
        assert_eq!(check_native_image_libc(&jar), Err(StaticLinkError::Jar));
        run(&[libc.display().to_string()]).unwrap_err();
    }
}
