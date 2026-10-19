use std::collections::HashSet;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{disp, fmt_size};

pub fn run(path_arg: Option<String>, assume_yes: bool, dry_run: bool, no_shutdown: bool) -> i32 {
    let distros = lxss_entries();
    let targets = match &path_arg {
        Some(p) => vec![crate::to_extended(Path::new(p))],
        None => find_vhdx(&distros),
    };

    if targets.is_empty() {
        println!("no WSL .vhdx disks found");
        return 0;
    }

    let mut total = 0u64;
    println!("WSL virtual disks:");
    for p in &targets {
        let sz = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        total += sz;
        println!("  {:>10}  {}", fmt_size(sz), disp(p));
    }
    println!("total on disk: {}\n", fmt_size(total));

    if dry_run {
        println!("--dry-run: nothing changed");
        return 0;
    }

    if !assume_yes && std::io::stdin().is_terminal() {
        use std::io::Write;
        print!("This will SHUT DOWN all WSL sessions, then compact the disks.\nContinue? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
        if !matches!(buf.trim().to_lowercase().as_str(), "y" | "yes" | "д" | "да") {
            println!("aborted");
            return 0;
        }
    }

    if !no_shutdown {
        println!("shutting down WSL...");
        match Command::new("wsl.exe").arg("--shutdown").status() {
            Ok(s) if !s.success() => eprintln!("  wsl --shutdown exited with {s}"),
            Err(e) => eprintln!("  cannot run wsl.exe: {e}"),
            _ => {}
        }
    }

    if path_arg.is_none() {
        for (name, _) in &distros {
            match Command::new("wsl.exe")
                .args(["--manage", name, "--set-sparse", "true"])
                .output()
            {
                Ok(o) if o.status.success() => {
                    println!("  {name}: sparse mode enabled (auto-shrinks in future)")
                }
                _ => {}
            }
        }
    }

    let mut freed = 0u64;
    for p in &targets {
        let before = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        println!("compacting {} ({})...", disp(p), fmt_size(before));
        match compact_vhdx(p) {
            Ok(()) => {
                let after = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let d = before.saturating_sub(after);
                freed += d;
                println!(
                    "  {} -> {}  (freed {})",
                    fmt_size(before),
                    fmt_size(after),
                    fmt_size(d)
                );
            }
            Err(code) => eprintln!("  failed: windows error {code} ({})", err_desc(code)),
        }
    }

    println!("\ntotal freed: {}", fmt_size(freed));
    0
}

fn err_desc(code: u32) -> &'static str {
    match code {
        5 => "access denied",
        32 => "file in use — is WSL really shut down?",
        50 => "not supported",
        87 => "invalid parameter",
        112 => "disk full",
        1223 => "canceled",
        _ => "",
    }
}

/// (distro name, base path) pairs from the WSL registry hive.
#[cfg(windows)]
fn lxss_entries() -> Vec<(String, PathBuf)> {
    let Ok(out) = Command::new("reg.exe")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss",
            "/s",
        ])
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut res = Vec::new();
    let mut name: Option<String> = None;
    let mut base: Option<PathBuf> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("HKEY") {
            if let (Some(n), Some(b)) = (name.take(), base.take()) {
                res.push((n, b));
            }
        } else if let Some(rest) = t.strip_prefix("DistributionName") {
            name = rest.split("REG_SZ").nth(1).map(|s| s.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("BasePath") {
            base = rest
                .split("REG_SZ")
                .nth(1)
                .map(|s| PathBuf::from(crate::to_extended(Path::new(s.trim()))));
        }
    }
    if let (Some(n), Some(b)) = (name, base) {
        res.push((n, b));
    }
    res
}

#[cfg(not(windows))]
fn lxss_entries() -> Vec<(String, PathBuf)> {
    Vec::new()
}

fn collect_vhdx(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<String>, depth: usize) {
    if depth > 5 {
        return;
    }
    let Ok(list) = fs::read_dir(dir) else { return };
    for e in list.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            collect_vhdx(&p, out, seen, depth + 1);
        } else if ft.is_file()
            && p.extension()
                .map(|x| x.eq_ignore_ascii_case("vhdx") || x.eq_ignore_ascii_case("vhd"))
                .unwrap_or(false)
            && seen.insert(p.to_string_lossy().to_lowercase())
        {
            out.push(p);
        }
    }
}

fn find_vhdx(distros: &[(String, PathBuf)]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // distro base paths from the registry (covers wsl --import locations too)
    for (_, base) in distros {
        collect_vhdx(base, &mut out, &mut seen, 0);
    }

    // fallback: Packages/*/LocalState and Docker/wsl under every user profile
    if let Ok(users) = fs::read_dir(crate::to_extended(Path::new(r"C:\Users"))) {
        for u in users.flatten() {
            let pkgs = u.path().join(r"AppData\Local\Packages");
            if let Ok(list) = fs::read_dir(&pkgs) {
                for p in list.flatten() {
                    collect_vhdx(&p.path().join("LocalState"), &mut out, &mut seen, 0);
                }
            }
            collect_vhdx(
                &u.path().join(r"AppData\Local\Docker\wsl"),
                &mut out,
                &mut seen,
                0,
            );
        }
    }

    out.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).map(|m| m.len()).unwrap_or(0)));
    out
}

#[cfg(windows)]
fn compact_vhdx(path: &Path) -> Result<(), u32> {
    vdisk::compact(path)
}

#[cfg(not(windows))]
fn compact_vhdx(_path: &Path) -> Result<(), u32> {
    Err(50) // ERROR_NOT_SUPPORTED
}

#[cfg(windows)]
mod vdisk {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    type Handle = *mut c_void;

    #[repr(C)]
    struct VirtualStorageType {
        device_id: u32,
        vendor_id: [u8; 16],
    }

    // VIRTUAL_STORAGE_TYPE: DeviceId=3 (VHDX), VendorId=VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT
    const VST_VHDX: VirtualStorageType = VirtualStorageType {
        device_id: 3,
        vendor_id: [
            0xec, 0x4a, 0x98, 0xec, 0xf9, 0xa0, 0xe9, 0x47, 0x90, 0x1f, 0x71, 0x41, 0x5a, 0x66,
            0x34, 0x5b,
        ],
    };

    #[repr(C)]
    struct OpenParams {
        version: u32,
        rw_depth: u32,
        _pad: [u32; 5], // union is 24 bytes in Version2 layout
    }

    const VIRTUAL_DISK_ACCESS_METAOPS: u32 = 0x0020_0000;
    const VIRTUAL_DISK_ACCESS_GET_INFO: u32 = 0x0008_0000;

    #[link(name = "virtdisk")]
    unsafe extern "system" {
        fn OpenVirtualDisk(
            vst: *const VirtualStorageType,
            path: *const u16,
            access_mask: u32,
            security_info: u32,
            flags: u32,
            params: *const OpenParams,
            handle: *mut Handle,
        ) -> u32;
        fn CompactVirtualDisk(
            handle: Handle,
            flags: u32,
            params: *const c_void,
            overlapped: *mut c_void,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(h: Handle) -> i32;
    }

    pub fn compact(path: &Path) -> Result<(), u32> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let params = OpenParams {
            version: 1,
            rw_depth: 1,
            _pad: [0; 5],
        };
        let mut h: Handle = ptr::null_mut();
        let err = unsafe {
            OpenVirtualDisk(
                &VST_VHDX,
                wide.as_ptr(),
                VIRTUAL_DISK_ACCESS_METAOPS | VIRTUAL_DISK_ACCESS_GET_INFO,
                0,
                0,
                &params,
                &mut h,
            )
        };
        if err != 0 {
            return Err(err);
        }
        let err = unsafe { CompactVirtualDisk(h, 0, ptr::null(), ptr::null_mut()) };
        unsafe { CloseHandle(h) };
        if err != 0 { Err(err) } else { Ok(()) }
    }
}
