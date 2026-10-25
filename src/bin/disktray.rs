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
static LAST_PCT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

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

/// load tray-<free%>.ico (pre-generated number icons in exe dir)
fn make_icon(free: u64, total: u64) -> HICON {
    let pct = (free as f64 / total.max(1) as f64 * 100.0).round().clamp(0.0, 100.0) as u32;
    let path = format!("{}\\tray-{}.ico", exe_dir(), pct);
    unsafe {
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
        let w = 110; // wide enough for "NN%" + bar
        // height -1: keep the taskbar's top border line visible
        Some((r.left - w, r.top, w, r.bottom - r.top - 1))
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
            MoveWindow(hw, x, y, w, h, 1);
            ShowWindow(hw, SW_SHOWNA);
            InvalidateRect(hw, std::ptr::null(), 1);
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
            let pct = LAST_PCT.load(std::sync::atomic::Ordering::Relaxed);
            // fill bar: bottom strip, width = pct% of w
            let bar = CreateSolidBrush(pct_color(pct));
            let br = Rect { left: 4, top: h - 7, right: 4 + ((w - 8) * pct as i32 / 100), bottom: h - 3 };
            FillRect(dc, &br, bar);
            DeleteObject(bar);
            // text "NN% free" — font height scales with taskbar height (= DPI-aware)
            let txt = format!("{}%  free", pct);
            SetBkMode(dc, 1); // TRANSPARENT
            SetTextColor(dc, 0x00FF_FFFF);
            let fh = -((h * 9 / 20).max(10)); // ~45% of taskbar height
            let font = CreateFontW(
                fh, 0, 0, 0, 600, 0, 0, 0, 0, 0, 0, 0, 0,
                wide("Segoe UI").as_ptr(),
            );
            let old = SelectObject(dc, font);
            let tr = Rect { left: 0, top: 2, right: w, bottom: h - 8 };
            DrawTextW(dc, wide(&txt).as_ptr(), -1, &tr as *const _ as *mut Rect, 0x25); // DT_CENTER|SINGLELINE|VCENTER
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
            update_tray(hwnd);
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
        update_tray(hwnd);

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
