use std::collections::HashSet;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{disp, fmt_size};

#[derive(Clone)]
pub struct DiskInfo {
    /// WSL distro name (empty when unknown / explicit file)
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

pub struct CompactReport {
    pub freed: u64,
    pub errors: Vec<String>,
}

/// Find all WSL/VM virtual disks.
pub fn discover() -> Vec<DiskInfo> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // distro base paths from the registry (covers wsl --import locations too)
    for (name, base) in lxss_entries() {
        let mut paths = Vec::new();
        collect_vhdx(&base, &mut paths, &mut seen, 0);
        for path in paths {
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            out.push(DiskInfo {
                name: name.clone(),
                path,
                size,
            });
        }
    }

    // fallback: Packages/*/LocalState and Docker/wsl under every user profile
    if let Ok(users) = fs::read_dir(crate::to_extended(Path::new(r"C:\Users"))) {
        for u in users.flatten() {
            let pkgs = u.path().join(r"AppData\Local\Packages");
            if let Ok(list) = fs::read_dir(&pkgs) {
                for p in list.flatten() {
                    let mut paths = Vec::new();
                    collect_vhdx(&p.path().join("LocalState"), &mut paths, &mut seen, 0);
                    for path in paths {
                        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        out.push(DiskInfo {
                            name: String::new(),
                            path,
                            size,
                        });
                    }
                }
            }
            let mut paths = Vec::new();
            collect_vhdx(
                &u.path().join(r"AppData\Local\Docker\wsl"),
                &mut paths,
                &mut seen,
                0,
            );
            for path in paths {
                let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                out.push(DiskInfo {
                    name: String::new(),
                    path,
                    size,
                });
            }
        }
    }

    out.sort_by(|a, b| b.size.cmp(&a.size));
    out
}

/// Shut down WSL, enable sparse mode, compact each selected disk.
/// `on_status` gets human-readable step text; `on_progress` gets 0..1 fraction.
pub fn execute(
    targets: &[DiskInfo],
    shutdown: bool,
    mut on_status: impl FnMut(&str),
    mut on_progress: impl FnMut(f64),
) -> CompactReport {
    let mut report = CompactReport {
        freed: 0,
        errors: Vec::new(),
    };

    if shutdown {
        on_status("shutting down WSL...");
        match Command::new("wsl.exe").arg("--shutdown").status() {
            Ok(s) if !s.success() => {
                report.errors.push(format!("wsl --shutdown exited {s}"))
            }
            Err(e) => report
                .errors
                .push(format!("cannot run wsl.exe --shutdown: {e}")),
            _ => {}
        }
    }

    for t in targets {
        if t.name.is_empty() {
            continue;
        }
        match Command::new("wsl.exe")
            .args(["--manage", &t.name, "--set-sparse", "true"])
            .output()
        {
            Ok(o) if o.status.success() => on_status(&format!(
                "{}: sparse mode enabled (auto-shrinks in future)",
                t.name
            )),
            _ => {}
        }
    }

    for (i, t) in targets.iter().enumerate() {
        let before = fs::metadata(&t.path).map(|m| m.len()).unwrap_or(t.size);
        on_status(&format!(
            "compacting {} ({})...",
            disp(&t.path),
            fmt_size(before)
        ));
        let base = i as f64 / targets.len() as f64;
        let span = 1.0 / targets.len() as f64;
        match compact_vhdx(&t.path, |f| on_progress(base + f * span)) {
            Ok(()) => {
                let after = fs::metadata(&t.path).map(|m| m.len()).unwrap_or(0);
                let d = before.saturating_sub(after);
                report.freed += d;
                on_status(&format!(
                    "  {} -> {}  (freed {})",
                    fmt_size(before),
                    fmt_size(after),
                    fmt_size(d)
                ));
            }
            Err(code) => {
                report
                    .errors
                    .push(format!("{}: error {code} {}", disp(&t.path), err_desc(code)));
            }
        }
        on_progress((i + 1) as f64 / targets.len() as f64);
    }
    report
}

/// CLI entry point (`diskhound compact`).
pub fn run(path_arg: Option<String>, assume_yes: bool, dry_run: bool, no_shutdown: bool) -> i32 {
    let targets = match &path_arg {
        Some(p) => {
            let path = crate::to_extended(Path::new(p));
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            vec![DiskInfo {
                name: String::new(),
                path,
                size,
            }]
        }
        None => discover(),
    };

    if targets.is_empty() {
        println!("no WSL .vhdx disks found");
        return 0;
    }

    let mut total = 0u64;
    println!("WSL virtual disks:");
    for t in &targets {
        total += t.size;
        let label = if t.name.is_empty() {
            disp(&t.path)
        } else {
            format!("{}  ({})", t.name, disp(&t.path))
        };
        println!("  {:>10}  {}", fmt_size(t.size), label);
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

    let report = execute(
        &targets,
        !no_shutdown,
        |s| println!("{s}"),
        |f| {
            use std::io::Write;
            print!("\r  overall: {:>5.1}%", f * 100.0);
            let _ = std::io::stdout().flush();
        },
    );

    println!();
    for e in &report.errors {
        eprintln!("error: {e}");
    }
    println!("\ntotal freed: {}", fmt_size(report.freed));
    if report.errors.is_empty() {
        0
    } else {
        1
    }
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

#[cfg(windows)]
fn compact_vhdx(path: &Path, progress: impl FnMut(f64)) -> Result<(), u32> {
    vdisk::compact(path, progress)
}

#[cfg(not(windows))]
fn compact_vhdx(_path: &Path, _progress: impl FnMut(f64)) -> Result<(), u32> {
    Err(50) // ERROR_NOT_SUPPORTED
}

#[cfg(windows)]
mod vdisk {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;
    use std::thread::sleep;
    use std::time::Duration;

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

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        h_event: Handle,
    }

    #[repr(C)]
    struct VirtualDiskProgress {
        operation_status: u32,
        current: u64,
        final_: u64,
    }

    const VIRTUAL_DISK_ACCESS_METAOPS: u32 = 0x0020_0000;
    const VIRTUAL_DISK_ACCESS_GET_INFO: u32 = 0x0008_0000;
    const ERROR_IO_PENDING: u32 = 997;

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
            overlapped: *mut Overlapped,
        ) -> u32;
        fn GetVirtualDiskOperationProgress(
            handle: Handle,
            overlapped: *mut Overlapped,
            progress: *mut VirtualDiskProgress,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(h: Handle) -> i32;
        fn CreateEventW(
            attrs: *const c_void,
            manual_reset: i32,
            initial_state: i32,
            name: *const u16,
        ) -> Handle;
    }

    pub fn compact(path: &Path, mut progress: impl FnMut(f64)) -> Result<(), u32> {
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
        let res = compact_async(h, &mut progress);
        unsafe { CloseHandle(h) };
        res
    }

    fn compact_async(h: Handle, progress: &mut impl FnMut(f64)) -> Result<(), u32> {
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return compact_sync(h);
        }
        let mut ov = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            h_event: event,
        };
        let call = unsafe { CompactVirtualDisk(h, 0, ptr::null(), &mut ov) };
        let res = if call == 0 {
            Ok(())
        } else if call == ERROR_IO_PENDING {
            loop {
                let mut prog = VirtualDiskProgress {
                    operation_status: ERROR_IO_PENDING,
                    current: 0,
                    final_: 0,
                };
                let st = unsafe { GetVirtualDiskOperationProgress(h, &mut ov, &mut prog) };
                if st == ERROR_IO_PENDING || prog.operation_status == ERROR_IO_PENDING {
                    if prog.final_ > 0 {
                        progress((prog.current as f64 / prog.final_ as f64).clamp(0.0, 1.0));
                    }
                    sleep(Duration::from_millis(200));
                    continue;
                }
                break if prog.operation_status == 0 {
                    progress(1.0);
                    Ok(())
                } else {
                    Err(prog.operation_status)
                };
            }
        } else {
            Err(call)
        };
        unsafe { CloseHandle(event) };
        res
    }

    fn compact_sync(h: Handle) -> Result<(), u32> {
        let err = unsafe { CompactVirtualDisk(h, 0, ptr::null(), ptr::null_mut()) };
        if err != 0 { Err(err) } else { Ok(()) }
    }
}
