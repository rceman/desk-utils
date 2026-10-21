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
const TPM_RIGHTBUTTON: u32 = 0x0002;
const MF_STRING: u32 = 0;
const ID_OPEN: usize = 1;
const ID_REFRESH: usize = 2;
const ID_EXIT: usize = 3;

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
    fn ReleaseDC(h: HWND, d: HDC) -> i32;
    fn MessageBoxW(h: HWND, t: *const u16, c: *const u16, f: u32) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn Shell_NotifyIconW(m: u32, d: *mut NotifyIconData) -> i32;
    fn ShellExecuteW(h: HWND, op: *const u16, f: *const u16, p: *const u16, d: *const u16, s: i32) -> isize;
}
#[link(name = "kernel32")]
unsafe extern "system" {
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

/// 16x16 icon: system-monitor style — framed box, fill from bottom = % used.
/// Fill color by free space: red <10%, yellow <20%, green <30%, blue above.
fn make_icon(free: u64, total: u64) -> HICON {
    let free_pct = free as f64 / total.max(1) as f64 * 100.0;
    let used = (1.0 - free as f64 / total.max(1) as f64).clamp(0.0, 1.0);
    let fill = if free_pct < 10.0 {
        0x0000_30D8 // red
    } else if free_pct < 20.0 {
        0x0000_C0F0 // yellow
    } else if free_pct < 30.0 {
        0x0030_B830 // green
    } else {
        0x00D8_7800 // blue (BGR ~ accent)
    };
    unsafe {
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);
        let bi = BitmapInfo {
            hdr: BitmapInfoHeader {
                size: 40,
                w: 16,
                h: -16,
                planes: 1,
                bpp: 32,
                compression: 3,
                size_image: 0,
                xppm: 0,
                yppm: 0,
                clr_used: 0,
                clr_important: 0,
            },
            colors: [0x00FF_0000, 0x0000_FF00, 0x0000_00FF],
        };
        let mut bits: *mut u32 = null_mut();
        let bmp = CreateDIBSection(dc, &bi, 3, &mut bits, null_mut(), 0);
        let old = SelectObject(dc, bmp);
        let bb = CreateSolidBrush(0x0028_2828);
        let fb = CreateSolidBrush(fill);
        let bp = CreatePen(0, 1, 0x00A0_A0A0);
        let ob = SelectObject(dc, bb);
        Rectangle(dc, 0, 0, 16, 16);
        let fill_h = (12.0 * used) as i32;
        SelectObject(dc, fb);
        Rectangle(dc, 2, 14 - fill_h, 14, 14);
        SelectObject(dc, bp);
        Rectangle(dc, 1, 1, 15, 15);
        SelectObject(dc, ob);
        SelectObject(dc, old);
        // GDI writes alpha=0 into 32bpp bitmaps -> force opaque
        if !bits.is_null() {
            for i in 0..256isize {
                *bits.offset(i) |= 0xFF00_0000;
            }
        }
        let zero = [0u8; 64]; // all-zero mask = fully opaque icon
        let mask = CreateBitmap(16, 16, 1, 1, zero.as_ptr() as *const c_void);
        let mut ii = IconInfo {
            icon: 1,
            x: 0,
            y: 0,
            mask,
            color: bmp,
        };
        let icon = CreateIconIndirect(&mut ii);
        DeleteObject(mask);
        DeleteObject(bmp);
        DeleteObject(fb);
        DeleteObject(bb);
        DeleteObject(bp);
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
        icon
    }
}

fn update_tray(hwnd: HWND) {
    if let Some((free, total)) = c_space() {
        let used = 1.0 - free as f64 / total.max(1) as f64;
        let tip = format!("C: {} GB free of {} GB ({:.0}% used)", fmt_gb(free), fmt_gb(total), used * 100.0);
        let mut d = NotifyIconData {
            cb_size: std::mem::size_of::<NotifyIconData>() as u32,
            hwnd,
            uid: 1,
            flags: NIF_ICON | NIF_TIP,
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
            if l == WM_RBUTTONUP as isize {
                let mut p = Point { x: 0, y: 0 };
                GetCursorPos(&mut p);
                let m = CreatePopupMenu();
                AppendMenuW(m, MF_STRING, ID_OPEN, wide("Open diskhound").as_ptr());
                AppendMenuW(m, MF_STRING, ID_REFRESH, wide("Refresh").as_ptr());
                AppendMenuW(m, MF_STRING, ID_EXIT, wide("Exit").as_ptr());
                SetForegroundWindow(hwnd);
                TrackPopupMenu(m, TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, std::ptr::null());
                DestroyMenu(m);
            }
            0
        }
        WM_COMMAND => match w {
            ID_OPEN => {
                ShellExecuteW(hwnd, std::ptr::null(), wide("diskhound.exe").as_ptr(), std::ptr::null(), std::ptr::null(), 5);
                0
            }
            ID_REFRESH => {
                update_tray(hwnd);
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
        let mut d = NotifyIconData {
            cb_size: std::mem::size_of::<NotifyIconData>() as u32,
            hwnd,
            uid: 1,
            flags: NIF_MESSAGE | NIF_ICON,
            msg: WM_TRAY,
            icon: make_icon(1, 1),
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
        Shell_NotifyIconW(NIM_ADD, &mut d);
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
