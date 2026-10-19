#![cfg(windows)]
use std::ffi::c_void;

// ---------- FFI ----------

pub(crate) type HWND = *mut c_void;
pub(crate) type HINSTANCE = *mut c_void;
pub(crate) type HMENU = *mut c_void;
pub(crate) type HFONT = *mut c_void;
pub(crate) type HBRUSH = *mut c_void;
pub(crate) type HICON = *mut c_void;
pub(crate) type HCURSOR = *mut c_void;
pub(crate) type HDC = *mut c_void;

#[repr(C)]
pub(crate) struct WndClassEx {
    pub(crate) cb_size: u32,
    pub(crate) style: u32,
    pub(crate) wnd_proc: Option<unsafe extern "system" fn(HWND, u32, usize, isize) -> isize>,
    pub(crate) cb_cls_extra: i32,
    pub(crate) cb_wnd_extra: i32,
    pub(crate) h_instance: HINSTANCE,
    pub(crate) h_icon: HICON,
    pub(crate) h_cursor: HCURSOR,
    pub(crate) hbr_background: HBRUSH,
    pub(crate) menu_name: *const u16,
    pub(crate) class_name: *const u16,
    pub(crate) h_icon_sm: HICON,
}

#[repr(C)]
pub(crate) struct Msg {
    pub(crate) hwnd: HWND,
    pub(crate) message: u32,
    pub(crate) w_param: usize,
    pub(crate) l_param: isize,
    pub(crate) time: u32,
    pub(crate) pt_x: i32,
    pub(crate) pt_y: i32,
    pub(crate) l_private: u32,
}

#[repr(C)]
pub(crate) struct Rect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[repr(C)]
pub(crate) struct PaintStruct {
    pub(crate) hdc: HDC,
    pub(crate) erase: i32,
    pub(crate) paint: Rect,
    pub(crate) restore: i32,
    pub(crate) inc_update: i32,
    pub(crate) reserved: [u8; 32],
}

#[repr(C)]
pub(crate) struct InitCtrls {
    pub(crate) size: u32,
    pub(crate) icc: u32,
}

#[repr(C)]
pub(crate) struct BrowseInfo {
    pub(crate) owner: HWND,
    pub(crate) pidl_root: *mut c_void,
    pub(crate) display_name: *mut u16,
    pub(crate) title: *const u16,
    pub(crate) flags: u32,
    pub(crate) callback: usize,
    pub(crate) lparam: isize,
    pub(crate) image: i32,
}

#[repr(C)]
pub(crate) struct ShFileOp {
    pub(crate) hwnd: HWND,
    pub(crate) func: u32,
    pub(crate) from: *const u16,
    pub(crate) to: *const u16,
    pub(crate) flags: u16,
    pub(crate) aborted: i32,
    pub(crate) name_maps: *mut c_void,
    pub(crate) title: *const u16,
}

pub(crate) const WM_CREATE: u32 = 0x0001;
pub(crate) const WM_DESTROY: u32 = 0x0002;
pub(crate) const WM_SIZE: u32 = 0x0005;
pub(crate) const WM_PAINT: u32 = 0x000F;
pub(crate) const WM_CLOSE: u32 = 0x0010;
pub(crate) const WM_CONTEXTMENU: u32 = 0x007B;
pub(crate) const WM_COMMAND: u32 = 0x0111;
pub(crate) const WM_TIMER: u32 = 0x0113;
pub(crate) const WM_APP_LOAD: u32 = 0x8000;
pub(crate) const WM_SETFONT: u32 = 0x0030;
pub(crate) const BM_GETCHECK: u32 = 0x00F0;
pub(crate) const BM_SETCHECK: u32 = 0x00F1;
pub(crate) const PBM_SETRANGE32: u32 = 0x0406;
pub(crate) const PBM_SETPOS: u32 = 0x0402;
pub(crate) const PBM_SETMARQUEE: u32 = 0x040A;
pub(crate) const LB_ADDSTRING: u32 = 0x0180;
pub(crate) const LB_RESETCONTENT: u32 = 0x0184;
pub(crate) const LB_SETSEL: u32 = 0x0185;
pub(crate) const LB_GETSEL: u32 = 0x0187;
pub(crate) const LB_GETSELCOUNT: u32 = 0x0190;
pub(crate) const LB_GETSELITEMS: u32 = 0x0191;
pub(crate) const LB_ITEMFROMPOINT: u32 = 0x01A9;
pub(crate) const CB_ADDSTRING: u32 = 0x0143;
pub(crate) const CB_SETCURSEL: u32 = 0x014E;
pub(crate) const CB_GETCURSEL: u32 = 0x0147;
pub(crate) const CBN_SELCHANGE: u32 = 1;
pub(crate) const LBN_DBLCLK: u32 = 2;

pub(crate) const WS_CHILD: u32 = 0x4000_0000;
pub(crate) const WS_VISIBLE: u32 = 0x1000_0000;
pub(crate) const WS_VSCROLL: u32 = 0x0020_0000;
pub(crate) const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub(crate) const WS_POPUPWINDOW: u32 = 0x8088_0000;
pub(crate) const WS_CAPTION: u32 = 0x00C0_0000;
pub(crate) const WS_SYSMENU: u32 = 0x0008_0000;
pub(crate) const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;

pub(crate) const BS_PUSHBUTTON: u32 = 0;
pub(crate) const BS_DEFPUSHBUTTON: u32 = 1;
pub(crate) const BS_AUTOCHECKBOX: u32 = 3;
pub(crate) const ES_MULTILINE: u32 = 0x0004;
pub(crate) const ES_READONLY: u32 = 0x0800;
pub(crate) const ES_AUTOVSCROLL: u32 = 0x0040;
pub(crate) const SS_LEFT: u32 = 0;
pub(crate) const PBS_MARQUEE: u32 = 0x0008;
pub(crate) const CBS_DROPDOWNLIST: u32 = 0x0003;
pub(crate) const LBS_NOTIFY: u32 = 0x0001;
pub(crate) const LBS_DISABLENOSCROLL: u32 = 0x2000;
pub(crate) const LBS_USETABSTOPS: u32 = 0x0080;
pub(crate) const LBS_EXTENDEDSEL: u32 = 0x0800;

pub(crate) const MF_STRING: u32 = 0;
pub(crate) const MF_POPUP: u32 = 0x10;
pub(crate) const MF_SEPARATOR: u32 = 0x800;

pub(crate) const MB_OK: u32 = 0;
pub(crate) const MB_YESNO: u32 = 4;
pub(crate) const MB_ICONWARNING: u32 = 0x30;
pub(crate) const MB_ICONINFORMATION: u32 = 0x40;
pub(crate) const MB_SETFOREGROUND: u32 = 0x0001_0000;
pub(crate) const IDYES: i32 = 6;

pub(crate) const FO_DELETE: u32 = 3;
pub(crate) const FOF_ALLOWUNDO: u16 = 0x0040;
pub(crate) const FOF_NOCONFIRMATION: u16 = 0x0010;
pub(crate) const TPM_RETURNCMD: u32 = 0x0100;
pub(crate) const CF_UNICODETEXT: u32 = 13;

pub(crate) const ID_SCAN_C: usize = 101;
pub(crate) const ID_SCAN_FOLDER: usize = 102;
pub(crate) const ID_COMPACT: usize = 103;
pub(crate) const ID_EXIT: usize = 104;
pub(crate) const ID_VIEW: usize = 105;
pub(crate) const ID_EXPLORER: usize = 106;
pub(crate) const ID_DELETE: usize = 107;
pub(crate) const ID_ADMIN: usize = 108;
pub(crate) const ID_LIST: usize = 109;
pub(crate) const ID_PICK_OK: usize = 300;
pub(crate) const ID_PICK_CANCEL: usize = 301;
pub(crate) const ID_PICK_CHECK0: usize = 400;
pub(crate) const ID_CTX_EXPLORER: usize = 500;
pub(crate) const ID_CTX_DELETE: usize = 501;
pub(crate) const ID_CTX_COPYPATH: usize = 502;
pub(crate) const TIMER_UI: usize = 1;

pub(crate) const LIST_ITEMS: usize = 200;

#[link(name = "user32")]
unsafe extern "system" {
    pub(crate) fn RegisterClassExW(class: *const WndClassEx) -> u16;
    pub(crate) fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HMENU,
        instance: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    pub(crate) fn DefWindowProcW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    pub(crate) fn GetMessageW(m: *mut Msg, h: HWND, min: u32, max: u32) -> i32;
    pub(crate) fn TranslateMessage(m: *const Msg) -> i32;
    pub(crate) fn DispatchMessageW(m: *const Msg) -> isize;
    pub(crate) fn PostQuitMessage(code: i32);
    pub(crate) fn DestroyWindow(h: HWND) -> i32;
    pub(crate) fn ShowWindow(h: HWND, cmd: i32) -> i32;
    pub(crate) fn UpdateWindow(h: HWND) -> i32;
    pub(crate) fn LoadCursorW(inst: HINSTANCE, name: *const u16) -> HCURSOR;
    pub(crate) fn LoadImageW(
        inst: HINSTANCE,
        name: *const u16,
        kind: u32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> HICON;
    pub(crate) fn SetWindowTextW(h: HWND, text: *const u16) -> i32;
    pub(crate) fn SendMessageW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    pub(crate) fn PostMessageW(h: HWND, m: u32, w: usize, l: isize) -> i32;
    pub(crate) fn SetTimer(h: HWND, id: usize, ms: u32, f: usize) -> usize;
    pub(crate) fn MoveWindow(h: HWND, x: i32, y: i32, w: i32, hh: i32, repaint: i32) -> i32;
    pub(crate) fn BeginPaint(h: HWND, p: *mut PaintStruct) -> HDC;
    pub(crate) fn EndPaint(h: HWND, p: *const PaintStruct) -> i32;
    pub(crate) fn CreateMenu() -> HMENU;
    pub(crate) fn AppendMenuW(m: HMENU, flags: u32, id: usize, text: *const u16) -> i32;
    pub(crate) fn SetMenu(h: HWND, m: HMENU) -> i32;
    pub(crate) fn MessageBoxW(h: HWND, text: *const u16, cap: *const u16, t: u32) -> i32;
    pub(crate) fn EnableWindow(h: HWND, en: i32) -> i32;
    pub(crate) fn GetWindowRect(h: HWND, r: *mut Rect) -> i32;
    pub(crate) fn SetProcessDPIAware() -> i32;
    pub(crate) fn SetForegroundWindow(h: HWND) -> i32;
    pub(crate) fn CreatePopupMenu() -> HMENU;
    pub(crate) fn DestroyMenu(m: HMENU) -> i32;
    pub(crate) fn TrackPopupMenu(
        m: HMENU,
        flags: u32,
        x: i32,
        y: i32,
        rsvd: i32,
        h: HWND,
        r: *const Rect,
    ) -> i32;
    pub(crate) fn ScreenToClient(h: HWND, x: *mut i32, y: *mut i32) -> i32;
    pub(crate) fn GetCursorPos(x: *mut i32, y: *mut i32) -> i32;
    pub(crate) fn OpenClipboard(h: HWND) -> i32;
    pub(crate) fn EmptyClipboard() -> i32;
    pub(crate) fn SetClipboardData(fmt: u32, h: usize) -> usize;
    pub(crate) fn CloseClipboard() -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    pub(crate) fn GetModuleFileNameW(h: HINSTANCE, path: *mut u16, size: u32) -> u32;
    pub(crate) fn GetCurrentProcess() -> *mut c_void;
    pub(crate) fn CloseHandle(h: *mut c_void) -> i32;
    pub(crate) fn GlobalAlloc(flags: u32, bytes: usize) -> usize;
    pub(crate) fn GlobalLock(h: usize) -> *mut c_void;
    pub(crate) fn GlobalUnlock(h: usize) -> i32;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    pub(crate) fn InitCommonControlsEx(ic: *const InitCtrls) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub(crate) fn CreateFontW(
        h: i32, w: i32, esc: i32, orient: i32, weight: i32, italic: u32, underline: u32,
        strike: u32, charset: u32, out_prec: u32, clip_prec: u32, quality: u32,
        pitch_fam: u32, face: *const u16,
    ) -> HFONT;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub(crate) fn SHBrowseForFolderW(b: *mut BrowseInfo) -> *mut c_void;
    pub(crate) fn SHGetPathFromIDListW(pidl: *mut c_void, path: *mut u16) -> i32;
    pub(crate) fn ShellExecuteW(
        hwnd: HWND,
        verb: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> usize;
    pub(crate) fn SHFileOperationW(op: *mut ShFileOp) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    pub(crate) fn CoInitializeEx(reserved: usize, model: u32) -> i32;
    pub(crate) fn CoTaskMemFree(p: *mut c_void);
}

#[link(name = "advapi32")]
unsafe extern "system" {
    pub(crate) fn OpenProcessToken(proc: *mut c_void, access: u32, tok: *mut *mut c_void) -> i32;
    pub(crate) fn GetTokenInformation(
        tok: *mut c_void,
        class: u32,
        info: *mut c_void,
        len: u32,
        retlen: *mut u32,
    ) -> i32;
}
