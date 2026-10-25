#![cfg(windows)]
#![windows_subsystem = "windows"]
//! disktray — system-tray indicator of free space on C:
//! icon = usage bar (green/orange/red), tooltip = exact numbers.
//! Right-click: Open diskhound / Refresh / Exit.

use std::ffi::c_void;
use std::ptr::null_mut;

type HWND = *mut c_void;
type HICON = *mut c_void;
type HMENU = *mut c_void;
type HBRUSH = *mut c_void;
type HDC = *mut c_void;

#[repr(C)]
struct WndClassEx {
    cb_size: u32,
    style: u32,
    wnd_proc: usize,
    cls_extra: i32,
    wnd_extra: i32,
    inst: *mut c_void,
    icon: HICON,
    cursor: *mut c_void,
    hbr_background: HBRUSH,
    menu_name: *const u16,
    class_name: *const u16,
    icon_sm: HICON,
}
#[repr(C)]
struct Msg {
    hwnd: HWND,
    message: u32,
    w: usize,
    l: isize,
    time: u32,
    x: i32,
    y: i32,
    priv_: i32,
}
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct NotifyIconData {
    cb_size: u32,
    hwnd: HWND,
    uid: u32,
    flags: u32,
    msg: u32,
    icon: HICON,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    timeout_or_version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid_item: [u8; 16],
    balloon_icon: HICON,
}
#[repr(C)]
struct BitmapInfo {
    hdr: BitmapInfoHeader,
    colors: [u32; 3],
}
#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    w: i32,
    h: i32,
    planes: u16,
    bpp: u16,
    compression: u32,
    size_image: u32,
    xppm: i32,
    yppm: i32,
    clr_used: u32,
    clr_important: u32,
}
#[repr(C)]
struct IconInfo {
    icon: i32,
    x: u32,
    y: u32,
    mask: *mut c_void,
    color: *mut c_void,
}

const WM_DESTROY: u32 = 0x0002;
const WM_TIMER: u32 = 0x0113;
const WM_COMMAND: u32 = 0x0111;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_TRAY: u32 = 0x8001;
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIF_ICON: u32 = 2;
const NIF_TIP: u32 = 4;
const NIF_MESSAGE: u32 = 1;
const NIF_SHOWTIP: u32 = 0x80;
const TPM_RIGHTBUTTON: u32 = 0x0002;
const MF_STRING: u32 = 0;
const MF_CHECKED: u32 = 8;
const ID_OPEN: usize = 1;
const ID_OPEN_ADMIN: usize = 4;
const ID_REFRESH: usize = 2;
const ID_EXIT: usize = 3;
const ID_WIDGET: usize = 5;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_TOPMOST: u32 = 0x8;
const WS_EX_TOOLWINDOW: u32 = 0x80;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const SW_HIDE: i32 = 0;
const SW_SHOWNA: i32 = 8;
const WM_PAINT: u32 = 0x000F;
const WM_LBUTTONUP: u32 = 0x0202;

static WIDGET_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static WIDGET_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static CFG_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static LAST_PCT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
// widget tuning (persisted to disktray.ini)
static W_WIDTH: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(200);
static W_XOFF: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static W_FONTPC: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(60);
static W_BARH: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(3);
// live metrics
static CPU_PCT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static RAM_PCT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static GPU_PCT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static GPU_TEMP: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static NVML_DEV: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static W_PCT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static PDH_Q: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PDH_C: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PREV_TIMES: std::sync::Mutex<(u64, u64, u64)> = std::sync::Mutex::new((0, 0, 0));

#[repr(C)]
struct MemoryStatus {
    len: u32,
    load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_pf: u64,
    avail_pf: u64,
    total_v: u64,
    avail_v: u64,
    ext_v: u64,
}
#[repr(C)]
struct FileTime {
    lo: u32,
    hi: u32,
}
#[repr(C)]
struct PdhValue {
    status: u32,
    _pad: u32,
    val: f64,
}
#[repr(C)]
struct PdhItem {
    name: *mut u16,
    fmt: PdhValue,
}

#[repr(C)]
struct PaintStruct {
    hdc: HDC,
    erase: i32,
    rc: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(w: *const WndClassEx) -> u16;
    fn CreateWindowExW(e: u32, c: *const u16, n: *const u16, s: u32, x: i32, y: i32, w: i32, h: i32, p: HWND, m: HMENU, i: *mut c_void, l: *mut c_void) -> HWND;
    fn DefWindowProcW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, h: HWND, mn: u32, mx: u32) -> i32;
    fn TranslateMessage(m: *const Msg) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
    fn PostQuitMessage(c: i32);
    fn SetTimer(h: HWND, id: usize, ms: u32, f: usize) -> usize;
    fn DestroyWindow(h: HWND) -> i32;
    fn GetModuleHandleW(n: *const u16) -> *mut c_void;
    fn LoadCursorW(i: *mut c_void, c: usize) -> *mut c_void;
    fn CreatePopupMenu() -> HMENU;
    fn AppendMenuW(m: HMENU, f: u32, id: usize, t: *const u16) -> i32;
    fn TrackPopupMenu(m: HMENU, f: u32, x: i32, y: i32, r: i32, h: HWND, rr: *const c_void) -> i32;
    fn DestroyMenu(m: HMENU) -> i32;
    fn GetCursorPos(p: *mut Point) -> i32;
    fn SetForegroundWindow(h: HWND) -> i32;
    fn GetDC(h: HWND) -> HDC;
    fn LoadImageW(i: *mut c_void, n: *const u16, t: u32, w: i32, h: i32, f: u32) -> HICON;
    fn GetModuleFileNameW(m: *mut c_void, b: *mut u16, n: u32) -> u32;
    fn ReleaseDC(h: HWND, d: HDC) -> i32;
    fn MessageBoxW(h: HWND, t: *const u16, c: *const u16, f: u32) -> i32;
    fn FindWindowW(c: *const u16, n: *const u16) -> HWND;
    fn FindWindowExW(p: HWND, ch: HWND, c: *const u16, n: *const u16) -> HWND;
    fn GetWindowRect(h: HWND, r: *mut Rect) -> i32;
    fn MoveWindow(h: HWND, x: i32, y: i32, w: i32, hh: i32, r: i32) -> i32;
    fn ShowWindow(h: HWND, c: i32) -> i32;
    fn InvalidateRect(h: HWND, r: *const Rect, e: i32) -> i32;
    fn BeginPaint(h: HWND, p: *mut PaintStruct) -> HDC;
    fn EndPaint(h: HWND, p: *const PaintStruct) -> i32;
    fn FillRect(d: HDC, r: *const Rect, b: HBRUSH) -> i32;
    fn SetProcessDpiAwarenessContext(c: isize) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn GetWindowTextW(h: HWND, b: *mut u16, n: i32) -> i32;
    fn SetWindowTextW(h: HWND, t: *const u16) -> i32;
    fn GetDlgItem(h: HWND, id: i32) -> HWND;
    fn SendMessageW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    fn SetWindowPos(h: HWND, ins: isize, x: i32, y: i32, w: i32, hh: i32, f: u32) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn Shell_NotifyIconW(m: u32, d: *mut NotifyIconData) -> i32;
    fn ShellExecuteW(h: HWND, op: *const u16, f: *const u16, p: *const u16, d: *const u16, s: i32) -> isize;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(a: *mut c_void, own: i32, n: *const u16) -> *mut c_void;
    fn GetLastError() -> u32;
    fn GetDiskFreeSpaceExW(d: *const u16, free_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
    fn GlobalMemoryStatusEx(m: *mut MemoryStatus) -> i32;
    fn GetSystemTimes(idle: *mut FileTime, kern: *mut FileTime, user: *mut FileTime) -> i32;
    fn LoadLibraryW(n: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, n: *const u8) -> *mut c_void;
}
#[link(name = "pdh")]
unsafe extern "system" {
    fn PdhOpenQueryW(d: *const u16, ud: usize, q: *mut *mut c_void) -> u32;
    fn PdhAddEnglishCounterW(q: *mut c_void, path: *const u16, ud: usize, c: *mut *mut c_void) -> u32;
    fn PdhCollectQueryData(q: *mut c_void) -> u32;
    fn PdhGetFormattedCounterArrayW(c: *mut c_void, fmt: u32, sz: *mut u32, cnt: *mut u32, buf: *mut u8) -> u32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(d: HDC) -> HDC;
    fn CreateDIBSection(d: HDC, i: *const BitmapInfo, u: u32, bits: *mut *mut u32, s: *mut c_void, off: u32) -> *mut c_void;
    fn CreateSolidBrush(c: u32) -> HBRUSH;
    fn CreatePen(s: i32, w: i32, c: u32) -> *mut c_void;
    fn SelectObject(d: HDC, o: *mut c_void) -> *mut c_void;
    fn Rectangle(d: HDC, l: i32, t: i32, r: i32, b: i32) -> i32;
    fn DeleteObject(o: *mut c_void) -> i32;
    fn DeleteDC(d: HDC) -> i32;
    fn CreateBitmap(w: i32, h: i32, p: u32, b: u32, bits: *const c_void) -> *mut c_void;
    fn CreateIconIndirect(i: *mut IconInfo) -> HICON;
    fn DestroyIcon(i: HICON) -> i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn DrawTextW(d: HDC, t: *const u16, n: i32, r: *mut Rect, f: u32) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateFontW(h: i32, w: i32, e: i32, o: i32, wt: i32, i: u32, u: u32, s: u32, cs: u32, op: u32, cp: u32, q: u32, pf: u32, f: *const u16) -> *mut c_void;
    fn SetTextColor(d: HDC, c: u32) -> u32;
    fn SetBkMode(d: HDC, m: i32) -> i32;
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn fmt_gb(b: u64) -> String {
    format!("{:.1}", b as f64 / 1_073_741_824.0)
}

/// (free bytes, total bytes)
fn c_space() -> Option<(u64, u64)> {
    let (mut free, mut total, mut tf) = (0u64, 0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(wide("C:\\").as_ptr(), &mut free, &mut total, &mut tf) };
    if ok != 0 {
        Some((free, total))
    } else {
        None
    }
}

fn exe_dir() -> String {
    let mut buf = [0u16; 600];
    let n = unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), 600) };
    let p = String::from_utf16_lossy(&buf[..n as usize]);
    std::path::Path::new(&p)
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string())
}

/// live bar-chart icon: 4 bars (CPU/RAM/GPU/C:) drawn pixel-wise
fn make_icon(free: u64, total: u64) -> HICON {
    let pct = (free as f64 / total.max(1) as f64 * 100.0).round().clamp(0.0, 100.0) as u32;
    // staircase: 4 ascending bars; lit count = free% level
    // lit = white (orange if <20%), unlit = gray
    let lit = ((100 - pct) / 25).min(4) as usize; // used% -> steps
    let (lc, uc) = if pct < 20 { (0xFF00_8CFFu32, 0xFF60_6060) } else { (0xFFFF_FFFF, 0xFF60_6060) };
    let bars = [
        (4u32, if lit >= 1 { lc } else { uc }),
        (7u32, if lit >= 2 { lc } else { uc }),
        (10u32, if lit >= 3 { lc } else { uc }),
        (13u32, if lit >= 4 { lc } else { uc }),
    ];
    unsafe {
        let bi = BitmapInfo {
            hdr: BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                w: 16, h: -16, // top-down
                planes: 1, bpp: 32, compression: 0,
                size_image: 0, xppm: 0, yppm: 0, clr_used: 0, clr_important: 0,
            },
            colors: [0; 3],
        };
        let mut bits: *mut u32 = null_mut();
        let bmp = CreateDIBSection(null_mut(), &bi, 0, &mut bits, null_mut(), 0);
        if !bmp.is_null() && !bits.is_null() {
            let px = std::slice::from_raw_parts_mut(bits, 256);
            for p in px.iter_mut() { *p = 0xFF20_2020; } // dark bg
            // monitor frame
            for i in 0..16 {
                px[i] = 0xFFA0_A0A0;
                px[15 * 16 + i] = 0xFFA0_A0A0;
                px[i * 16] = 0xFFA0_A0A0;
                px[i * 16 + 15] = 0xFFA0_A0A0;
            }
            // 4 bars, evenly spread across the frame (x = 2,5,8,11 + 3px wide)
            for (i, (v, col)) in bars.iter().enumerate() {
                let bh = *v as usize;
                let x0 = 2 + i * 3;
                for y in (15 - bh)..15 {
                    for x in x0..(x0 + 2) {
                        px[y * 16 + x] = *col;
                    }
                }
            }
            let zero = [0u8; 64];
            let mask = CreateBitmap(16, 16, 1, 1, zero.as_ptr() as *const c_void);
            let mut ii = IconInfo { icon: 1, x: 0, y: 0, mask, color: bmp };
            let h = CreateIconIndirect(&mut ii);
            DeleteObject(mask); DeleteObject(bmp);
            if !h.is_null() { return h; }
        }
        if !bmp.is_null() { DeleteObject(bmp); }
        // fallback: pre-generated percent icon
        let path = format!("{}\\tray-{}.ico", exe_dir(), pct);
        LoadImageW(null_mut(), wide(&path).as_ptr(), 1, 16, 16, 0x10)
    }
}

// ---- taskbar widget: small window parked left of TrayNotifyWnd ----

fn widget_rect() -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        if tray.is_null() {
            return None;
        }
        let notify = FindWindowExW(tray, null_mut(), wide("TrayNotifyWnd").as_ptr(), std::ptr::null());
        if notify.is_null() {
            return None;
        }
        let mut r = Rect { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(notify, &mut r) == 0 {
            return None;
        }
        use std::sync::atomic::Ordering::Relaxed;
        let w = W_WIDTH.load(Relaxed);
        let xoff = W_XOFF.load(Relaxed);
        // inset: keep taskbar borders visible, slimmer than full height
        Some((r.left - w + xoff, r.top + 4, w, r.bottom - r.top - 8))
    }
}

fn park_widget() {
    use std::sync::atomic::Ordering::Relaxed;
    let hw = WIDGET_HWND.load(Relaxed) as HWND;
    if hw.is_null() {
        return;
    }
    if !WIDGET_ON.load(Relaxed) {
        unsafe { ShowWindow(hw, SW_HIDE) };
        return;
    }
    if let Some((x, y, w, h)) = widget_rect() {
        unsafe {
            // re-assert TOPMOST every call: the taskbar raises itself above
            // our window on clicks (SWP_NOACTIVATE|SWP_SHOWWINDOW)
            SetWindowPos(hw, -1, x, y, w, h, 0x10 | 0x40);
            InvalidateRect(hw, std::ptr::null(), 1);
        }
    }
}

fn ft_u64(f: &FileTime) -> u64 {
    ((f.hi as u64) << 32) | f.lo as u64
}

fn init_pdh() {
    use std::sync::atomic::Ordering::Relaxed;
    unsafe {
        let mut q: *mut c_void = null_mut();
        if PdhOpenQueryW(std::ptr::null(), 0, &mut q) != 0 {
            return;
        }
        let mut c: *mut c_void = null_mut();
        if PdhAddEnglishCounterW(
            q,
            wide("\\GPU Engine(*)\\Utilization Percentage").as_ptr(),
            0,
            &mut c,
        ) != 0
        {
            return;
        }
        PdhCollectQueryData(q);
        PDH_Q.store(q as usize, Relaxed);
        PDH_C.store(c as usize, Relaxed);
    }
}

type NvmlTemp = unsafe extern "system" fn(*mut c_void, u32, *mut u32) -> u32;
static NVML_TEMP_FN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn init_nvml() {
    use std::sync::atomic::Ordering::Relaxed;
    unsafe {
        for p in ["nvml.dll", "C:\\Windows\\System32\\nvml.dll"] {
            let m = LoadLibraryW(wide(p).as_ptr());
            if m.is_null() {
                continue;
            }
            type Init = unsafe extern "system" fn() -> u32;
            type ByIdx = unsafe extern "system" fn(u32, *mut *mut c_void) -> u32;
            let init: Init = std::mem::transmute(GetProcAddress(m, b"nvmlInit_v2\0".as_ptr()));
            let byidx: ByIdx = std::mem::transmute(GetProcAddress(m, b"nvmlDeviceGetHandleByIndex_v2\0".as_ptr()));
            let temp: NvmlTemp = std::mem::transmute(GetProcAddress(m, b"nvmlDeviceGetTemperature\0".as_ptr()));
            let i = init as usize;
            let b = byidx as usize;
            let t = temp as usize;
            if i != 0 && b != 0 && t != 0 && init() == 0 {
                let mut dev: *mut c_void = null_mut();
                if byidx(0, &mut dev) == 0 {
                    NVML_DEV.store(dev as usize, Relaxed);
                    NVML_TEMP_FN.store(t, Relaxed);
                }
            }
            return;
        }
    }
}

fn collect_metrics() {
    use std::sync::atomic::Ordering::Relaxed;
    unsafe {
        // RAM
        let mut m = MemoryStatus {
            len: std::mem::size_of::<MemoryStatus>() as u32,
            load: 0, total_phys: 0, avail_phys: 0,
            total_pf: 0, avail_pf: 0, total_v: 0, avail_v: 0, ext_v: 0,
        };
        if GlobalMemoryStatusEx(&mut m) != 0 {
            RAM_PCT.store(m.load, Relaxed);
        }
        // CPU: delta of GetSystemTimes
        let (mut i, mut k, mut u) = (
            FileTime { lo: 0, hi: 0 },
            FileTime { lo: 0, hi: 0 },
            FileTime { lo: 0, hi: 0 },
        );
        if GetSystemTimes(&mut i, &mut k, &mut u) != 0 {
            let (ci, ck, cu) = (ft_u64(&i), ft_u64(&k), ft_u64(&u));
            let mut p = PREV_TIMES.lock().unwrap();
            if p.0 != 0 {
                let (di, dk, du) = (ci - p.0, ck - p.1, cu - p.2);
                let busy = dk + du - di;
                let total = dk + du;
                if total > 0 {
                    CPU_PCT.store(((busy * 100) / total) as u32, Relaxed);
                }
            }
            *p = (ci, ck, cu);
        }
        // W: free %
        let (mut free, mut total, mut tf) = (0u64, 0u64, 0u64);
        if GetDiskFreeSpaceExW(wide("W:\\").as_ptr(), &mut free, &mut total, &mut tf) != 0 {
            W_PCT.store(
                (free as f64 / total.max(1) as f64 * 100.0).round() as i32,
                Relaxed,
            );
        } else {
            W_PCT.store(-1, Relaxed);
        }
        // GPU temp via NVML
        let (dev, tf) = (NVML_DEV.load(Relaxed), NVML_TEMP_FN.load(Relaxed));
        if dev != 0 && tf != 0 {
            let f: NvmlTemp = std::mem::transmute(tf);
            let mut t = 0u32;
            if f(dev as *mut c_void, 0, &mut t) == 0 {
                GPU_TEMP.store(t as i32, Relaxed);
            }
        }
        // GPU via PDH
        let (q, c) = (PDH_Q.load(Relaxed) as *mut c_void, PDH_C.load(Relaxed) as *mut c_void);
        if !q.is_null() && !c.is_null() && PdhCollectQueryData(q) == 0 {
            let (mut sz, mut cnt) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(c, 0x200, &mut sz, &mut cnt, null_mut())
                == 0x8000_07D2
            {
                let mut buf = vec![0u8; sz as usize];
                if PdhGetFormattedCounterArrayW(c, 0x200, &mut sz, &mut cnt, buf.as_mut_ptr()) == 0 {
                    let items = std::slice::from_raw_parts(buf.as_ptr() as *const PdhItem, cnt as usize);
                    let mut mx = 0f64;
                    for it in items {
                        if it.fmt.status == 0 && it.fmt.val > mx {
                            mx = it.fmt.val;
                        }
                    }
                    GPU_PCT.store(mx.round().clamp(0.0, 100.0) as i32, Relaxed);
                }
            }
        }
    }
}

fn pct_color(pct: u32) -> u32 {
    // BGR
    if pct < 10 { 0x0030_30D8 }        // red
    else if pct < 20 { 0x003A_A6CA }   // muted yellow
    else if pct < 30 { 0x0030_B830 }   // green
    else { 0x00D8_7800 }               // blue
}

unsafe extern "system" fn widget_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        WM_PAINT => {
            let mut ps = PaintStruct {
                hdc: null_mut(), erase: 0,
                rc: Rect { left: 0, top: 0, right: 0, bottom: 0 },
                restore: 0, inc_update: 0, reserved: [0; 32],
            };
            let dc = BeginPaint(hwnd, &mut ps);
            let mut rc = Rect { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut rc);
            let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
            // bg ~ taskbar dark
            let bg = CreateSolidBrush(0x0023_2323);
            let all = Rect { left: 0, top: 0, right: w, bottom: h };
            FillRect(dc, &all, bg);
            DeleteObject(bg);
            use std::sync::atomic::Ordering::Relaxed;
            let pct = LAST_PCT.load(Relaxed);
            // grid: CPU/RAM/GPU labels on top row, values bottom row;
            // last column = C: (top) and W: (bottom)
            let (cpu, ram) = (CPU_PCT.load(Relaxed), RAM_PCT.load(Relaxed));
            let gpu = GPU_PCT.load(Relaxed);
            let wp = W_PCT.load(Relaxed);
            let gt = GPU_TEMP.load(Relaxed);
            let gtxt = if gpu >= 0 && gt >= 0 { format!("{}% {}°", gpu, gt) }
                       else if gt >= 0 { format!("{}°", gt) }
                       else if gpu >= 0 { format!("{}%", gpu) } else { "—".into() };
            SetBkMode(dc, 1); // TRANSPARENT
            let fh = -(((h - 4) / 2 * W_FONTPC.load(Relaxed) / 100).max(8));
            let font = CreateFontW(
                fh, 0, 0, 0, 600, 0, 0, 0, 0, 0, 0, 0, 0,
                wide("Segoe UI").as_ptr(),
            );
            let old = SelectObject(dc, font);
            let rows_h = h;
            let dim = 0x00A0_A0A0u32;
            let warn = 0x0000_8CFFu32; // orange (BGR)
            let white = 0x00FF_FFFFu32;
            // 5 equal cols: CPU RAM GPU C: W: — values aligned on one row
            let cw = w / 5;
            let cells: [(&str, String, u32); 5] = [
                ("CPU", format!("{}%", cpu), if cpu >= 80 { warn } else { white }),
                ("RAM", format!("{}%", ram), if ram >= 80 { warn } else { white }),
                ("GPU", gtxt, if gpu >= 80 { warn } else { white }),
                ("C:/", format!("{}%", pct), if pct < 20 { warn } else { white }),
                ("W:/", if wp >= 0 { format!("{}%", wp) } else { "—".into() },
                 if wp < 0 { dim } else if wp < 20 { warn } else { white }),
            ];
            for (i, (lbl, val, col)) in cells.iter().enumerate() {
                let x0 = i as i32 * cw;
                let x1 = if i == 4 { w } else { x0 + cw };
                let mut rt = Rect { left: x0, top: 0, right: x1, bottom: rows_h / 2 };
                let mut rb = Rect { left: x0, top: rows_h / 2, right: x1, bottom: rows_h };
                SetTextColor(dc, dim);
                DrawTextW(dc, wide(lbl).as_ptr(), -1, &mut rt, 0x25);
                SetTextColor(dc, *col);
                DrawTextW(dc, wide(val).as_ptr(), -1, &mut rb, 0x25);
            }
            SelectObject(dc, old);
            DeleteObject(font);
            EndPaint(hwnd, &ps);
            0
        }
        WM_LBUTTONUP => {
            let exe = format!("{}\\diskhound.exe", exe_dir());
            ShellExecuteW(hwnd, std::ptr::null(), wide(&exe).as_ptr(), std::ptr::null(), std::ptr::null(), 5);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

// ---- settings window ----

fn ini_path() -> String {
    format!("{}\\disktray.ini", exe_dir())
}

fn load_settings() {
    use std::sync::atomic::Ordering::Relaxed;
    if let Ok(s) = std::fs::read_to_string(ini_path()) {
        for line in s.lines() {
            let mut kv = line.splitn(2, '=');
            let (k, v) = (kv.next().unwrap_or(""), kv.next().unwrap_or(""));
            let Ok(n) = v.trim().parse::<i32>() else { continue };
            match k.trim() {
                "w" => W_WIDTH.store(n, Relaxed),
                "x" => W_XOFF.store(n, Relaxed),
                "f" => W_FONTPC.store(n, Relaxed),
                "b" => W_BARH.store(n, Relaxed),
                _ => {}
            }
        }
    }
}

fn save_settings() {
    use std::sync::atomic::Ordering::Relaxed;
    let s = format!(
        "w={}\nx={}\nf={}\nb={}\n",
        W_WIDTH.load(Relaxed),
        W_XOFF.load(Relaxed),
        W_FONTPC.load(Relaxed),
        W_BARH.load(Relaxed)
    );
    let _ = std::fs::write(ini_path(), s);
}

unsafe extern "system" fn cfg_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    use std::sync::atomic::Ordering::Relaxed;
    match msg {
        WM_COMMAND => match w & 0xFFFF {
            210 => {
                // Apply
                for (id, cell) in [
                    (200i32, &W_WIDTH),
                    (201, &W_XOFF),
                    (202, &W_FONTPC),
                    (203, &W_BARH),
                ] {
                    let e = GetDlgItem(hwnd, id);
                    let mut buf = [0u16; 32];
                    GetWindowTextW(e, buf.as_mut_ptr(), 32);
                    let end = buf.iter().position(|&c| c == 0).unwrap_or(32);
                    let s = String::from_utf16_lossy(&buf[..end]);
                    if let Ok(n) = s.trim().parse::<i32>() {
                        cell.store(n, Relaxed);
                    }
                }
                save_settings();
                park_widget();
                0
            }
            211 => {
                DestroyWindow(hwnd);
                CFG_HWND.store(0, Relaxed);
                0
            }
            _ => 0,
        },
        WM_DESTROY => {
            CFG_HWND.store(0, Relaxed);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn open_settings(owner: HWND) {
    use std::sync::atomic::Ordering::Relaxed;
    unsafe {
        let cur = CFG_HWND.load(Relaxed) as HWND;
        if !cur.is_null() {
            SetForegroundWindow(cur);
            return;
        }
        let inst = GetModuleHandleW(std::ptr::null_mut());
        let c = wide("disktray_cfg");
        let wc = WndClassEx {
            cb_size: std::mem::size_of::<WndClassEx>() as u32,
            style: 0,
            wnd_proc: cfg_proc as usize,
            cls_extra: 0,
            wnd_extra: 0,
            inst,
            icon: null_mut(),
            cursor: LoadCursorW(null_mut(), 32512),
            hbr_background: 16 as HBRUSH,
            menu_name: std::ptr::null(),
            class_name: c.as_ptr(),
            icon_sm: null_mut(),
        };
        RegisterClassExW(&wc);
        let h = CreateWindowExW(
            0,
            c.as_ptr(),
            wide("disktray widget").as_ptr(),
            0x00CF_0000, // WS_OVERLAPPEDWINDOW
            -2147483648, -2147483648, 260, 220,
            owner, null_mut(), inst, null_mut(),
        );
        let font = CreateFontW(-14, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, wide("Segoe UI").as_ptr());
        let mk = |x: i32, y: i32, w: i32, hh: i32, txt: &str, cls: &str, id: usize, st: u32| -> HWND {
            let e = CreateWindowExW(
                st, wide(cls).as_ptr(), wide(txt).as_ptr(),
                0x5000_0000, // WS_CHILD|WS_VISIBLE
                x, y, w, hh, h, id as HMENU, inst, null_mut(),
            );
            SendMessageW(e, 0x30, font as usize, 1);
            e
        };
        let vals = [
            W_WIDTH.load(Relaxed).to_string(),
            W_XOFF.load(Relaxed).to_string(),
            W_FONTPC.load(Relaxed).to_string(),
            W_BARH.load(Relaxed).to_string(),
        ];
        let rows = [("Width", &vals[0], 200), ("X offset", &vals[1], 201), ("Font %", &vals[2], 202), ("Bar h", &vals[3], 203)];
        for (i, (lbl, val, id)) in rows.iter().enumerate() {
            let y = 12 + i as i32 * 28;
            mk(12, y, 90, 20, lbl, "STATIC", 0, 0);
            mk(110, y - 2, 60, 22, val, "EDIT", *id, 0x800);
        }
        mk(30, 128, 90, 26, "Apply", "BUTTON", 210, 0);
        mk(130, 128, 90, 26, "Close", "BUTTON", 211, 0);
        ShowWindow(h, 5);
        CFG_HWND.store(h as usize, Relaxed);
    }
}

fn update_tray(hwnd: HWND) {
    let cs = c_space();

    if let Some((free, total)) = cs {
        LAST_PCT.store(
            (free as f64 / total.max(1) as f64 * 100.0).round().clamp(0.0, 100.0) as u32,
            std::sync::atomic::Ordering::Relaxed,
        );
        park_widget();
        let used = 1.0 - free as f64 / total.max(1) as f64;
        let tip = format!("C: {} GB free of {} GB ({:.0}% used)", fmt_gb(free), fmt_gb(total), used * 100.0);
        let mut d = NotifyIconData {
            cb_size: std::mem::size_of::<NotifyIconData>() as u32,
            hwnd,
            uid: 1,
            flags: NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            msg: WM_TRAY,
            icon: make_icon(free, total),
            tip: [0u16; 128],
            state: 0,
            state_mask: 0,
            info: [0u16; 256],
            timeout_or_version: 0,
            info_title: [0u16; 64],
            info_flags: 0,
            guid_item: [0u8; 16],
            balloon_icon: null_mut(),
        };
        for (i, c) in tip.encode_utf16().take(127).enumerate() {
            d.tip[i] = c;
        }
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &mut d);
            if d.icon as usize != 0 {
                DestroyIcon(d.icon);
            }
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        WM_TIMER => {
            if w == 2 {
                collect_metrics();
                update_tray(hwnd);
                park_widget();
                let wh = WIDGET_HWND.load(std::sync::atomic::Ordering::Relaxed) as HWND;
                if !wh.is_null() {
                    unsafe { InvalidateRect(wh, std::ptr::null(), 0) };
                }
            } else {
                update_tray(hwnd);
            }
            0
        }
        WM_TRAY => {
            // v4: lParam = (uID << 16) | mouse event
            let ev = (l & 0xFFFF) as u32;
            if ev == 0x0202 {
                // left click -> open diskhound
                let exe = format!("{}\\diskhound.exe", exe_dir());
                unsafe {
                    ShellExecuteW(hwnd, std::ptr::null(), wide(&exe).as_ptr(), std::ptr::null(), std::ptr::null(), 5);
                }
            }
            if ev == WM_RBUTTONUP {
                let mut p = Point { x: 0, y: 0 };
                GetCursorPos(&mut p);
                let m = CreatePopupMenu();
                AppendMenuW(m, MF_STRING, ID_OPEN, wide("Open diskhound").as_ptr());
                AppendMenuW(m, MF_STRING, ID_OPEN_ADMIN, wide("Open as administrator").as_ptr());
                AppendMenuW(m, MF_STRING, ID_REFRESH, wide("Refresh").as_ptr());
                let wf = if WIDGET_ON.load(std::sync::atomic::Ordering::Relaxed) { MF_CHECKED } else { 0 };
                AppendMenuW(m, wf, ID_WIDGET, wide("Taskbar widget").as_ptr());
                AppendMenuW(m, MF_STRING, 6, wide("Widget settings...").as_ptr());
                AppendMenuW(m, MF_STRING, ID_EXIT, wide("Exit").as_ptr());
                SetForegroundWindow(hwnd);
                TrackPopupMenu(m, TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, std::ptr::null());
                DestroyMenu(m);
            }
            0
        }
        WM_COMMAND => match w {
            ID_OPEN => {
                let exe = format!("{}\\diskhound.exe", exe_dir());
                ShellExecuteW(hwnd, std::ptr::null(), wide(&exe).as_ptr(), std::ptr::null(), std::ptr::null(), 5);
                0
            }
            ID_OPEN_ADMIN => {
                let exe = format!("{}\\diskhound.exe", exe_dir());
                unsafe {
                    ShellExecuteW(hwnd, wide("runas").as_ptr(), wide(&exe).as_ptr(), std::ptr::null(), std::ptr::null(), 5);
                }
                0
            }
            ID_REFRESH => {
                update_tray(hwnd);
                0
            }
            ID_WIDGET => {
                let cur = WIDGET_ON.load(std::sync::atomic::Ordering::Relaxed);
                WIDGET_ON.store(!cur, std::sync::atomic::Ordering::Relaxed);
                park_widget();
                0
            }
            6 => {
                open_settings(hwnd);
                0
            }
            ID_EXIT => {
                let mut d = NotifyIconData {
                    cb_size: std::mem::size_of::<NotifyIconData>() as u32,
                    hwnd,
                    uid: 1,
                    flags: 0,
                    msg: 0,
                    icon: null_mut(),
                    tip: [0u16; 128],
                    state: 0,
                    state_mask: 0,
                    info: [0u16; 256],
                    timeout_or_version: 0,
                    info_title: [0u16; 64],
                    info_flags: 0,
                    guid_item: [0u8; 16],
                    balloon_icon: null_mut(),
                };
                Shell_NotifyIconW(NIM_DELETE, &mut d);
                DestroyWindow(hwnd);
                0
            }
            _ => 0,
        },
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

pub fn main() {
    unsafe {
        // per-monitor v2: sharp rendering at any DPI (else the window is
        // bitmap-scaled by the system and text looks blurry)
        if SetProcessDpiAwarenessContext(-4) == 0 {
            SetProcessDPIAware();
        }
        CreateMutexW(std::ptr::null_mut(), 1, wide("disktray-mutex").as_ptr());
        if GetLastError() == 183 {
            return; // already running
        }
    }
    let inst = unsafe { GetModuleHandleW(std::ptr::null_mut()) };
    let class = wide("disktray");
    let wc = WndClassEx {
        cb_size: std::mem::size_of::<WndClassEx>() as u32,
        style: 0,
        wnd_proc: wnd_proc as usize,
        cls_extra: 0,
        wnd_extra: 0,
        inst,
        icon: null_mut(),
        cursor: unsafe { LoadCursorW(null_mut(), 32512) },
        hbr_background: null_mut(),
        menu_name: std::ptr::null(),
        class_name: class.as_ptr(),
        icon_sm: null_mut(),
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            MessageBoxW(null_mut(), wide("register failed").as_ptr(), wide("disktray").as_ptr(), 0);
            return;
        }
        // hidden message window
        let hwnd = CreateWindowExW(0, class.as_ptr(), wide("disktray").as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), inst, null_mut());
        // taskbar widget window class + instance
        let wclass = wide("disktray_widget");
        let wwc = WndClassEx {
            cb_size: std::mem::size_of::<WndClassEx>() as u32,
            style: 0,
            wnd_proc: widget_proc as usize,
            cls_extra: 0,
            wnd_extra: 0,
            inst,
            icon: null_mut(),
            cursor: LoadCursorW(null_mut(), 32512),
            hbr_background: null_mut(),
            menu_name: std::ptr::null(),
            class_name: wclass.as_ptr(),
            icon_sm: null_mut(),
        };
        RegisterClassExW(&wwc);
        let wh = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            wclass.as_ptr(),
            wide("").as_ptr(),
            WS_POPUP,
            0, 0, 0, 0,
            null_mut(), null_mut(), inst, null_mut(),
        );
        WIDGET_HWND.store(wh as usize, std::sync::atomic::Ordering::Relaxed);
        let mut d = NotifyIconData {
            cb_size: std::mem::size_of::<NotifyIconData>() as u32,
            hwnd,
            uid: 1,
            flags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            msg: WM_TRAY,
            icon: make_icon(1, 1),
            tip: {
                let mut t = [0u16; 128];
                for (i, c) in "diskhound C:\\".encode_utf16().enumerate() {
                    t[i] = c;
                }
                t
            },
            state: 0,
            state_mask: 0,
            info: [0u16; 256],
            timeout_or_version: 0,
            info_title: [0u16; 64],
            info_flags: 0,
            guid_item: [0u8; 16],
            balloon_icon: null_mut(),
        };
        Shell_NotifyIconW(NIM_ADD, &mut d);
        // v4 -> WM_TRAY lParam carries the mouse message directly
        d.flags = 0;
        d.timeout_or_version = 4;
        Shell_NotifyIconW(4 /* NIM_SETVERSION */, &mut d);
        SetTimer(hwnd, 1, 60_000, 0);
        SetTimer(hwnd, 2, 1_000, 0);
        load_settings();
        init_pdh();
        init_nvml();
        update_tray(hwnd);
        collect_metrics();

        let mut msg = Msg {
            hwnd: null_mut(),
            message: 0,
            w: 0,
            l: 0,
            time: 0,
            x: 0,
            y: 0,
            priv_: 0,
        };
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
