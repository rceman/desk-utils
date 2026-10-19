#![cfg(windows)]
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::scan::{ScanData, ScanProgress};
use super::ffi::*;

// ---------- shared state ----------

pub(crate) struct Shared {
    pub(crate) busy: bool,
    pub(crate) status: String,
    pub(crate) scan_prog: Option<Arc<ScanProgress>>,
    pub(crate) result: Option<String>,
    pub(crate) done_msg: Option<String>,
    pub(crate) progress_pct: Option<f64>,
    pub(crate) data: Option<Arc<ScanData>>,
    pub(crate) list: Option<Vec<(String, PathBuf)>>,
    pub(crate) list_paths: Vec<PathBuf>,
    pub(crate) view: usize, // 0=files 1=dirs (list content)
    pub(crate) last_root: PathBuf,
    pub(crate) elevated: bool,
    pub(crate) dark: bool,
}

pub(crate) static SHARED: Mutex<Shared> = Mutex::new(Shared {
    busy: false,
    status: String::new(),
    scan_prog: None,
    result: None,
    done_msg: None,
    progress_pct: None,
    data: None,
    list: None,
    list_paths: Vec::new(),
    view: 0,
    last_root: PathBuf::new(),
    elevated: false,
    dark: false,
});

pub(crate) struct Windows {
    pub(crate) main: usize,
    pub(crate) edit: usize,
    pub(crate) list: usize,
    pub(crate) status: usize,
    pub(crate) prog: usize,
    pub(crate) admin_btn: usize,
    pub(crate) combo: usize,
    pub(crate) kids: Vec<usize>,
}

pub(crate) static WNDS: Mutex<Windows> = Mutex::new(Windows {
    main: 0,
    edit: 0,
    list: 0,
    status: 0,
    prog: 0,
    admin_btn: 0,
    combo: 0,
    kids: Vec::new(),
});

pub(crate) struct PickState {
    pub(crate) checks: Vec<usize>,
    pub(crate) result: Option<Vec<bool>>,
    pub(crate) closed: bool,
}

pub(crate) static PICK: Mutex<PickState> = Mutex::new(PickState {
    checks: Vec::new(),
    result: None,
    closed: true,
});

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn set_status(s: &str) {
    SHARED.lock().unwrap().status = s.to_string();
}

pub(crate) fn exe_dir() -> Option<PathBuf> {
    let mut buf = vec![0u16; 600];
    let n = unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), 600) };
    if n == 0 {
        return None;
    }
    let p = String::from_utf16_lossy(&buf[..n as usize]);
    std::path::Path::new(&p).parent().map(|d| d.to_path_buf())
}

pub(crate) fn cache_path(root: &std::path::Path) -> PathBuf {
    let s = root.to_string_lossy();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    exe_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!("diskhound-{:08x}.cache", h & 0xFFFF_FFFF))
}

pub(crate) fn set_progress(f: f64) {
    SHARED.lock().unwrap().progress_pct = Some(f);
}

pub(crate) fn is_elevated() -> bool {
    unsafe {
        let mut tok: *mut c_void = std::ptr::null_mut();
        // TOKEN_QUERY = 8, TokenElevation = 20
        if OpenProcessToken(GetCurrentProcess(), 8, &mut tok) == 0 {
            return false;
        }
        let mut elev = 0u32;
        let mut rl = 0u32;
        let ok = GetTokenInformation(tok, 20, &mut elev as *mut u32 as *mut c_void, 4, &mut rl);
        CloseHandle(tok);
        ok != 0 && elev != 0
    }
}

