#![cfg(windows)]

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::compact::{self, DiskInfo};
use crate::scan::{self, ScanData, ScanProgress};
use crate::{disp, fmt_num, fmt_size};

// ---------- FFI ----------

type HWND = *mut c_void;
type HINSTANCE = *mut c_void;
type HMENU = *mut c_void;
type HFONT = *mut c_void;
type HBRUSH = *mut c_void;
type HICON = *mut c_void;
type HCURSOR = *mut c_void;
type HDC = *mut c_void;

#[repr(C)]
struct WndClassEx {
    cb_size: u32,
    style: u32,
    wnd_proc: Option<unsafe extern "system" fn(HWND, u32, usize, isize) -> isize>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: HINSTANCE,
    h_icon: HICON,
    h_cursor: HCURSOR,
    hbr_background: HBRUSH,
    menu_name: *const u16,
    class_name: *const u16,
    h_icon_sm: HICON,
}

#[repr(C)]
struct Msg {
    hwnd: HWND,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt_x: i32,
    pt_y: i32,
    l_private: u32,
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct PaintStruct {
    hdc: HDC,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
struct InitCtrls {
    size: u32,
    icc: u32,
}

#[repr(C)]
struct BrowseInfo {
    owner: HWND,
    pidl_root: *mut c_void,
    display_name: *mut u16,
    title: *const u16,
    flags: u32,
    callback: usize,
    lparam: isize,
    image: i32,
}

#[repr(C)]
struct ShFileOp {
    hwnd: HWND,
    func: u32,
    from: *const u16,
    to: *const u16,
    flags: u16,
    aborted: i32,
    name_maps: *mut c_void,
    title: *const u16,
}

const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0005;
const WM_PAINT: u32 = 0x000F;
const WM_CLOSE: u32 = 0x0010;
const WM_CONTEXTMENU: u32 = 0x007B;
const WM_COMMAND: u32 = 0x0111;
const WM_TIMER: u32 = 0x0113;
const WM_SETFONT: u32 = 0x0030;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const PBM_SETRANGE32: u32 = 0x0406;
const PBM_SETPOS: u32 = 0x0402;
const PBM_SETMARQUEE: u32 = 0x040A;
const LB_ADDSTRING: u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_SETSEL: u32 = 0x0185;
const LB_GETSEL: u32 = 0x0187;
const LB_GETSELCOUNT: u32 = 0x0190;
const LB_GETSELITEMS: u32 = 0x0191;
const LB_ITEMFROMPOINT: u32 = 0x01A9;
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETCURSEL: u32 = 0x0147;
const CBN_SELCHANGE: u32 = 1;
const LBN_DBLCLK: u32 = 2;

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_POPUPWINDOW: u32 = 0x8088_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;

const BS_PUSHBUTTON: u32 = 0;
const BS_DEFPUSHBUTTON: u32 = 1;
const BS_AUTOCHECKBOX: u32 = 3;
const ES_MULTILINE: u32 = 0x0004;
const ES_READONLY: u32 = 0x0800;
const ES_AUTOVSCROLL: u32 = 0x0040;
const SS_LEFT: u32 = 0;
const PBS_MARQUEE: u32 = 0x0008;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const LBS_NOTIFY: u32 = 0x0001;
const LBS_DISABLENOSCROLL: u32 = 0x2000;
const LBS_USETABSTOPS: u32 = 0x0080;
const LBS_EXTENDEDSEL: u32 = 0x0800;

const MF_STRING: u32 = 0;
const MF_POPUP: u32 = 0x10;
const MF_SEPARATOR: u32 = 0x800;

const MB_OK: u32 = 0;
const MB_YESNO: u32 = 4;
const MB_ICONWARNING: u32 = 0x30;
const MB_ICONINFORMATION: u32 = 0x40;
const MB_SETFOREGROUND: u32 = 0x0001_0000;
const IDYES: i32 = 6;

const FO_DELETE: u32 = 3;
const FOF_ALLOWUNDO: u16 = 0x0040;
const FOF_NOCONFIRMATION: u16 = 0x0010;
const TPM_RETURNCMD: u32 = 0x0100;
const CF_UNICODETEXT: u32 = 13;

const ID_SCAN_C: usize = 101;
const ID_SCAN_FOLDER: usize = 102;
const ID_COMPACT: usize = 103;
const ID_EXIT: usize = 104;
const ID_VIEW: usize = 105;
const ID_EXPLORER: usize = 106;
const ID_DELETE: usize = 107;
const ID_ADMIN: usize = 108;
const ID_LIST: usize = 109;
const ID_PICK_OK: usize = 300;
const ID_PICK_CANCEL: usize = 301;
const ID_PICK_CHECK0: usize = 400;
const ID_CTX_EXPLORER: usize = 500;
const ID_CTX_DELETE: usize = 501;
const ID_CTX_COPYPATH: usize = 502;
const TIMER_UI: usize = 1;

const LIST_ITEMS: usize = 200;

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WndClassEx) -> u16;
    fn CreateWindowExW(
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
    fn DefWindowProcW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, h: HWND, min: u32, max: u32) -> i32;
    fn TranslateMessage(m: *const Msg) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
    fn PostQuitMessage(code: i32);
    fn DestroyWindow(h: HWND) -> i32;
    fn ShowWindow(h: HWND, cmd: i32) -> i32;
    fn UpdateWindow(h: HWND) -> i32;
    fn LoadCursorW(inst: HINSTANCE, name: *const u16) -> HCURSOR;
    fn SetWindowTextW(h: HWND, text: *const u16) -> i32;
    fn SendMessageW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    fn SetTimer(h: HWND, id: usize, ms: u32, f: usize) -> usize;
    fn MoveWindow(h: HWND, x: i32, y: i32, w: i32, hh: i32, repaint: i32) -> i32;
    fn BeginPaint(h: HWND, p: *mut PaintStruct) -> HDC;
    fn EndPaint(h: HWND, p: *const PaintStruct) -> i32;
    fn CreateMenu() -> HMENU;
    fn AppendMenuW(m: HMENU, flags: u32, id: usize, text: *const u16) -> i32;
    fn SetMenu(h: HWND, m: HMENU) -> i32;
    fn MessageBoxW(h: HWND, text: *const u16, cap: *const u16, t: u32) -> i32;
    fn EnableWindow(h: HWND, en: i32) -> i32;
    fn GetWindowRect(h: HWND, r: *mut Rect) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn SetForegroundWindow(h: HWND) -> i32;
    fn CreatePopupMenu() -> HMENU;
    fn TrackPopupMenu(
        m: HMENU,
        flags: u32,
        x: i32,
        y: i32,
        rsvd: i32,
        h: HWND,
        r: *const Rect,
    ) -> i32;
    fn ScreenToClient(h: HWND, x: *mut i32, y: *mut i32) -> i32;
    fn GetCursorPos(x: *mut i32, y: *mut i32) -> i32;
    fn OpenClipboard(h: HWND) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(fmt: u32, h: usize) -> usize;
    fn CloseClipboard() -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    fn GetModuleFileNameW(h: HINSTANCE, path: *mut u16, size: u32) -> u32;
    fn GetCurrentProcess() -> *mut c_void;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> usize;
    fn GlobalLock(h: usize) -> *mut c_void;
    fn GlobalUnlock(h: usize) -> i32;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn InitCommonControlsEx(ic: *const InitCtrls) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateFontW(
        h: i32, w: i32, esc: i32, orient: i32, weight: i32, italic: u32, underline: u32,
        strike: u32, charset: u32, out_prec: u32, clip_prec: u32, quality: u32,
        pitch_fam: u32, face: *const u16,
    ) -> HFONT;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHBrowseForFolderW(b: *mut BrowseInfo) -> *mut c_void;
    fn SHGetPathFromIDListW(pidl: *mut c_void, path: *mut u16) -> i32;
    fn ShellExecuteW(
        hwnd: HWND,
        verb: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> usize;
    fn SHFileOperationW(op: *mut ShFileOp) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: usize, model: u32) -> i32;
    fn CoTaskMemFree(p: *mut c_void);
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(proc: *mut c_void, access: u32, tok: *mut *mut c_void) -> i32;
    fn GetTokenInformation(
        tok: *mut c_void,
        class: u32,
        info: *mut c_void,
        len: u32,
        retlen: *mut u32,
    ) -> i32;
}

// ---------- shared state ----------

struct Shared {
    busy: bool,
    status: String,
    scan_prog: Option<Arc<ScanProgress>>,
    result: Option<String>,
    done_msg: Option<String>,
    progress_pct: Option<f64>,
    data: Option<Arc<ScanData>>,
    list: Option<Vec<(String, PathBuf)>>,
    list_paths: Vec<PathBuf>,
    view: usize, // 0=files 1=dirs (list content)
    last_root: PathBuf,
    elevated: bool,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
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
});

struct Windows {
    main: usize,
    edit: usize,
    list: usize,
    status: usize,
    prog: usize,
    admin_btn: usize,
    combo: usize,
}

static WNDS: Mutex<Windows> = Mutex::new(Windows {
    main: 0,
    edit: 0,
    list: 0,
    status: 0,
    prog: 0,
    admin_btn: 0,
    combo: 0,
});

struct PickState {
    checks: Vec<usize>,
    result: Option<Vec<bool>>,
    closed: bool,
}

static PICK: Mutex<PickState> = Mutex::new(PickState {
    checks: Vec::new(),
    result: None,
    closed: true,
});

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_status(s: &str) {
    SHARED.lock().unwrap().status = s.to_string();
}

fn set_progress(f: f64) {
    SHARED.lock().unwrap().progress_pct = Some(f);
}

pub fn is_elevated() -> bool {
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

// ---------- render dispatch ----------

fn refresh_list() {
    let (data, view) = {
        let g = SHARED.lock().unwrap();
        match &g.data {
            Some(d) => (d.clone(), g.view),
            None => return,
        }
    };
    thread::spawn(move || {
        let items = if view == 0 {
            scan::render_files(&data, LIST_ITEMS)
        } else {
            scan::render_dirs(&data, LIST_ITEMS)
        };
        let mut g = SHARED.lock().unwrap();
        g.list_paths = items.iter().map(|i| i.1.clone()).collect();
        g.list = Some(items);
    });
}

// ---------- workers ----------

fn start_scan(root: PathBuf) {
    let prog = Arc::new(ScanProgress::default());
    {
        let mut g = SHARED.lock().unwrap();
        if g.busy {
            return;
        }
        g.busy = true;
        g.last_root = root.clone();
        g.scan_prog = Some(prog.clone());
        g.result = None;
        g.list = None;
        g.progress_pct = None;
        g.status = format!("scanning {} ...", disp(&root));
    }

    let main_hwnd = WNDS.lock().unwrap().main;
    unsafe {
        SendMessageW(WNDS.lock().unwrap().prog as HWND, PBM_SETMARQUEE, 1, 30);
    }

    thread::spawn(move || {
        let data = scan::scan(root, Some(prog));
        let summary = format!(
            "{} — {}  ({} dirs, {} files, {:.1}s)",
            disp(&data.root),
            fmt_size(data.root_total),
            fmt_num(data.dirs.len() as u64),
            fmt_num(data.total_files),
            data.elapsed
        );
        let data = Arc::new(data);
        let admin_hint = {
            let mut g = SHARED.lock().unwrap();
            g.data = Some(data.clone());
            g.status = summary;
            g.scan_prog = None;
            g.busy = false;
            !g.elevated
        };
        let tree_text = scan::render(&data, 4, 8, 25, 0.2, admin_hint).replace('\n', "\r\n");
        SHARED.lock().unwrap().result = Some(tree_text);
        refresh_list();
        unsafe {
            SendMessageW(main_hwnd as HWND, WM_TIMER, TIMER_UI, 0);
        }
    });
}

fn start_compact(selected: Vec<DiskInfo>) {
    let main_hwnd = WNDS.lock().unwrap().main;
    thread::spawn(move || {
        let report = compact::execute(
            &selected,
            true,
            |s| set_status(s),
            |f| {
                set_progress(f);
                set_status(&format!("compacting... {:.0}%", f * 100.0));
            },
        );
        let mut msg = format!("Done. Freed: {}", fmt_size(report.freed));
        if !report.errors.is_empty() {
            msg.push_str(&format!("\n\nErrors:\n- {}", report.errors.join("\n- ")));
        }
        {
            let mut g = SHARED.lock().unwrap();
            g.done_msg = Some(msg);
            g.status = format!("compact finished — freed {}", fmt_size(report.freed));
            g.busy = false;
            g.progress_pct = None;
        }
        unsafe {
            SendMessageW(main_hwnd as HWND, WM_TIMER, TIMER_UI, 0);
        }
    });
}

// ---------- disk picker ----------

fn pick_disks(parent: HWND, disks: &[DiskInfo]) -> Option<Vec<DiskInfo>> {
    let inst = unsafe { GetModuleHandleW(std::ptr::null_mut()) };
    let class = wide("diskhound_pick");
    let wc = WndClassEx {
        cb_size: std::mem::size_of::<WndClassEx>() as u32,
        style: 0,
        wnd_proc: Some(pick_proc),
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: inst,
        h_icon: std::ptr::null_mut(),
        h_cursor: unsafe { LoadCursorW(std::ptr::null_mut(), 32512usize as *const u16) },
        hbr_background: 16 as HBRUSH, // COLOR_BTNFACE + 1
        menu_name: std::ptr::null(),
        class_name: class.as_ptr(),
        h_icon_sm: std::ptr::null_mut(),
    };
    unsafe { RegisterClassExW(&wc) };

    let n = disks.len() as i32;
    let win_w = 720i32;
    let win_h = n * 26 + 120;

    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(parent, &mut r) };
    let x = r.left + ((r.right - r.left) - win_w) / 2;
    let y = r.top + ((r.bottom - r.top) - win_h) / 2;

    {
        let mut p = PICK.lock().unwrap();
        p.checks.clear();
        p.result = None;
        p.closed = false;
    }

    let title = wide("Compact WSL disks");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUPWINDOW | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            x,
            y,
            win_w,
            win_h,
            parent,
            std::ptr::null_mut(),
            inst,
            std::ptr::null_mut(),
        )
    };
    if hwnd.is_null() {
        set_status("picker window failed to create");
        unsafe { EnableWindow(parent, 1) };
        return None;
    }

    let hint = format!("Found {} WSL disk(s). Select which to compact:", disks.len());
    unsafe {
        CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide(&hint).as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            12,
            10,
            win_w - 24,
            20,
            hwnd,
            std::ptr::null_mut(),
            inst,
            std::ptr::null_mut(),
        );
    }

    {
        let mut p = PICK.lock().unwrap();
        for (i, d) in disks.iter().enumerate() {
            let label = if d.name.is_empty() {
                format!("{}  ({})", disp(&d.path), fmt_size(d.size))
            } else {
                format!("{} — {}  ({})", d.name, disp(&d.path), fmt_size(d.size))
            };
            let cb = unsafe {
                CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide(&label).as_ptr(),
                    WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX,
                    12,
                    36 + i as i32 * 26,
                    win_w - 30,
                    22,
                    hwnd,
                    (ID_PICK_CHECK0 + i) as HMENU,
                    inst,
                    std::ptr::null_mut(),
                )
            };
            unsafe { SendMessageW(cb, BM_SETCHECK, 1, 0) };
            p.checks.push(cb as usize);
        }
    }

    let by = 36 + n * 26 + 14;
    unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide("Compact selected").as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            win_w - 320,
            by,
            150,
            30,
            hwnd,
            ID_PICK_OK as HMENU,
            inst,
            std::ptr::null_mut(),
        );
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide("Cancel").as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
            win_w - 158,
            by,
            140,
            30,
            hwnd,
            ID_PICK_CANCEL as HMENU,
            inst,
            std::ptr::null_mut(),
        );
        EnableWindow(parent, 0);
        ShowWindow(hwnd, 5);
        UpdateWindow(hwnd);
    }

    loop {
        if PICK.lock().unwrap().closed {
            break;
        }
        let mut msg = Msg {
            hwnd: std::ptr::null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            pt_x: 0,
            pt_y: 0,
            l_private: 0,
        };
        let r = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if r <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe { EnableWindow(parent, 1) };
    let result = PICK.lock().unwrap().result.take();
    result.map(|flags| {
        disks
            .iter()
            .zip(flags.iter())
            .filter(|(_, f)| **f)
            .map(|(d, _)| d.clone())
            .collect()
    })
}

unsafe extern "system" fn pick_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        WM_COMMAND => {
            let id = w & 0xFFFF;
            match id {
                ID_PICK_OK => {
                    let mut p = PICK.lock().unwrap();
                    let mut flags = Vec::with_capacity(p.checks.len());
                    for cb in &p.checks {
                        flags.push(unsafe {
                            SendMessageW(*cb as HWND, BM_GETCHECK, 0, 0) == 1
                        });
                    }
                    p.result = Some(flags);
                    p.closed = true;
                    unsafe { DestroyWindow(hwnd) };
                }
                ID_PICK_CANCEL => {
                    PICK.lock().unwrap().closed = true;
                    unsafe { DestroyWindow(hwnd) };
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            PICK.lock().unwrap().closed = true;
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            PICK.lock().unwrap().closed = true;
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

// ---------- actions ----------

fn selected_paths() -> Vec<PathBuf> {
    let wnds = WNDS.lock().unwrap();
    if wnds.list == 0 {
        return Vec::new();
    }
    let lb = wnds.list as HWND;
    let n = unsafe { SendMessageW(lb, LB_GETSELCOUNT, 0, 0) };
    if n <= 0 {
        return Vec::new();
    }
    let mut idx = vec![0i32; n as usize];
    unsafe {
        SendMessageW(lb, LB_GETSELITEMS, n as usize, idx.as_mut_ptr() as isize);
    }
    let g = SHARED.lock().unwrap();
    idx.iter()
        .filter_map(|i| g.list_paths.get(*i as usize).cloned())
        .collect()
}

fn open_in_explorer(path: &PathBuf) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", disp(path)))
        .spawn();
}

fn delete_to_recycle(hwnd: HWND, paths: &[PathBuf]) {
    let labels: Vec<String> = paths.iter().map(|p| disp(p)).collect();
    let shown: Vec<&String> = labels.iter().take(8).collect();
    let mut text = format!("Send {} item(s) to Recycle Bin?\n\n", labels.len());
    for s in &shown {
        text.push_str(&format!("  {s}\n"));
    }
    if labels.len() > shown.len() {
        text.push_str(&format!("  ... {} more\n", labels.len() - shown.len()));
    }
    let r = unsafe {
        MessageBoxW(
            hwnd,
            wide(&text).as_ptr(),
            wide("diskhound — delete").as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND,
        )
    };
    if r != IDYES {
        return;
    }
    // multi-string buffer: path1\0path2\0\0
    let mut from: Vec<u16> = Vec::new();
    for s in &labels {
        from.extend(s.encode_utf16().chain(std::iter::once(0)));
    }
    from.push(0);
    let mut op = ShFileOp {
        hwnd,
        func: FO_DELETE,
        from: from.as_ptr(),
        to: std::ptr::null(),
        flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION,
        aborted: 0,
        name_maps: std::ptr::null_mut(),
        title: std::ptr::null(),
    };
    let res = unsafe { SHFileOperationW(&mut op) };
    if res == 0 {
        set_status(&format!("moved {} item(s) to recycle bin — rescanning", labels.len()));
        let root = SHARED.lock().unwrap().last_root.clone();
        start_scan(root);
    } else {
        set_status(&format!("delete failed (shell error {res})"));
    }
}

fn copy_path_to_clipboard(hwnd: HWND, text: &str) {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }
        EmptyClipboard();
        let buf = wide(text);
        let h = GlobalAlloc(0x0042, buf.len() * 2); // GMEM_MOVEABLE|ZEROINIT
        if h != 0 {
            let p = GlobalLock(h);
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(buf.as_ptr(), p as *mut u16, buf.len());
                GlobalUnlock(h);
                SetClipboardData(CF_UNICODETEXT, h);
            }
        }
        CloseClipboard();
    }
    set_status("path copied to clipboard");
}

fn relaunch_admin(hwnd: HWND) {
    let mut buf = vec![0u16; 600];
    let n = unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), 600) };
    if n == 0 {
        return;
    }
    buf.truncate(n as usize);
    let r = unsafe {
        ShellExecuteW(
            hwnd,
            wide("runas").as_ptr(),
            buf.as_ptr(),
            wide("--gui").as_ptr(),
            std::ptr::null(),
            1,
        )
    };
    if r > 32 {
        unsafe { DestroyWindow(hwnd) };
    } else {
        set_status(&format!("elevation failed (error {r})"));
    }
}

fn on_action(hwnd: HWND, id: usize, notify: u32) {
    // combo selection change -> switch view + re-render
    if id == ID_VIEW && notify == CBN_SELCHANGE {
        let sel = unsafe {
            SendMessageW(WNDS.lock().unwrap().combo as HWND, CB_GETCURSEL, 0, 0)
        };
        if sel >= 0 {
            SHARED.lock().unwrap().view = sel as usize;
            refresh_list();
        }
        return;
    }
    // listbox double-click -> open in explorer
    if id == ID_LIST && notify == LBN_DBLCLK {
        if let Some(p) = selected_paths().first().cloned() {
            open_in_explorer(&p);
        }
        return;
    }
    if notify != 0 {
        return;
    }
    let busy = SHARED.lock().unwrap().busy;
    match id {
        ID_SCAN_C => {
            if !busy {
                start_scan(crate::to_extended(&PathBuf::from(r"C:\")));
            }
        }
        ID_SCAN_FOLDER => {
            if busy {
                return;
            }
            if let Some(dir) = browse_folder(hwnd) {
                start_scan(crate::to_extended(&dir));
            }
        }
        ID_COMPACT => {
            if busy {
                return;
            }
            set_status("looking for WSL disks...");
            let disks = compact::discover();
            if disks.is_empty() {
                set_status("ready");
                unsafe {
                    MessageBoxW(
                        hwnd,
                        wide("No WSL .vhdx disks found.").as_ptr(),
                        wide("diskhound").as_ptr(),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
                return;
            }
            set_status(&format!("found {} WSL disk(s)", disks.len()));
            match pick_disks(hwnd, &disks) {
                None => set_status("ready"),
                Some(sel) if sel.is_empty() => set_status("nothing selected"),
                Some(sel) => {
                    let names: Vec<String> = sel
                        .iter()
                        .map(|d| {
                            if d.name.is_empty() {
                                disp(&d.path)
                            } else {
                                d.name.clone()
                            }
                        })
                        .collect();
                    let total: u64 = sel.iter().map(|d| d.size).sum();
                    let text = format!(
                        "Selected:\n  - {}\nTotal on disk: {}\n\nThis will SHUT DOWN all running WSL sessions.\nContinue?",
                        names.join("\n  - "),
                        fmt_size(total)
                    );
                    let r = unsafe {
                        MessageBoxW(
                            hwnd,
                            wide(&text).as_ptr(),
                            wide("Compact WSL disks").as_ptr(),
                            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND,
                        )
                    };
                    if r == IDYES {
                        {
                            let mut g = SHARED.lock().unwrap();
                            g.busy = true;
                            g.progress_pct = Some(0.0);
                            g.status = "starting compact...".into();
                        }
                        unsafe {
                            SendMessageW(
                                WNDS.lock().unwrap().prog as HWND,
                                PBM_SETMARQUEE,
                                1,
                                30,
                            );
                        }
                        start_compact(sel);
                    } else {
                        set_status("ready");
                    }
                }
            }
        }
        ID_EXPLORER => match selected_paths().first().cloned() {
            Some(p) => open_in_explorer(&p),
            None => set_status("select an item in the largest list first"),
        },
        ID_DELETE => {
            let sel = selected_paths();
            if sel.is_empty() {
                set_status("select an item in the largest list first");
            } else {
                delete_to_recycle(hwnd, &sel);
            }
        }
        ID_ADMIN => {
            if !is_elevated() {
                relaunch_admin(hwnd);
            }
        }
        ID_EXIT => unsafe {
            DestroyWindow(hwnd);
        },
        _ => {}
    }
}

fn context_menu(hwnd: HWND, lparam: isize) {
    let wnds = WNDS.lock().unwrap();
    let lb = wnds.list as HWND;
    // lParam = screen coords (LOWORD x, HIWORD y), -1/-1 when invoked via keyboard
    let (sx, sy) = if lparam == -1 {
        (0i32, 0i32)
    } else {
        ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32)
    };
    // pick item under cursor; keep existing multi-selection if it contains it
    if lparam != -1 {
        let (mut cx, mut cy) = (sx, sy);
        unsafe { ScreenToClient(lb, &mut cx, &mut cy) };
        let r = unsafe {
            SendMessageW(
                lb,
                LB_ITEMFROMPOINT,
                0,
                ((cy & 0xFFFF) << 16 | (cx & 0xFFFF)) as isize,
            )
        };
        let idx = (r & 0xFFFF) as i32;
        if r >= 0 && idx >= 0 {
            let sel = unsafe { SendMessageW(lb, LB_GETSEL, idx as usize, 0) };
            if sel == 0 {
                unsafe {
                    SendMessageW(lb, LB_SETSEL, 0, -1isize);
                    SendMessageW(lb, LB_SETSEL, 1, idx as isize);
                }
            }
        }
    }
    let paths = selected_paths();
    if paths.is_empty() {
        return;
    }
    let menu = unsafe { CreatePopupMenu() };
    unsafe {
        AppendMenuW(menu, MF_STRING, ID_CTX_EXPLORER, wide("Open in Explorer").as_ptr());
        AppendMenuW(
            menu,
            MF_STRING,
            ID_CTX_COPYPATH,
            wide("Copy path").as_ptr(),
        );
        let label = if paths.len() == 1 {
            "Send to Recycle Bin".to_string()
        } else {
            format!("Send {} items to Recycle Bin", paths.len())
        };
        AppendMenuW(menu, MF_STRING, ID_CTX_DELETE, wide(&label).as_ptr());
        let (mx, my) = if lparam == -1 {
            let (mut gx, mut gy) = (0i32, 0i32);
            GetCursorPos(&mut gx, &mut gy);
            (gx, gy)
        } else {
            (sx, sy)
        };
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD,
            mx,
            my,
            0,
            hwnd,
            std::ptr::null(),
        );
        DestroyWindow(menu);
        match cmd as usize {
            ID_CTX_EXPLORER => open_in_explorer(&paths[0]),
            ID_CTX_COPYPATH => copy_path_to_clipboard(hwnd, &disp(&paths[0])),
            ID_CTX_DELETE => delete_to_recycle(hwnd, &paths),
            _ => {}
        }
    }
}

fn browse_folder(hwnd: HWND) -> Option<PathBuf> {
    let mut buf = vec![0u16; 512];
    let title = wide("Select folder to scan");
    let mut bi = BrowseInfo {
        owner: hwnd,
        pidl_root: std::ptr::null_mut(),
        display_name: buf.as_mut_ptr(),
        title: title.as_ptr(),
        flags: 0x0001 | 0x0040, // RETURNONLYFSDIRS | NEWDIALOGSTYLE
        callback: 0,
        lparam: 0,
        image: 0,
    };
    let pidl = unsafe { SHBrowseForFolderW(&mut bi) };
    if pidl.is_null() {
        return None;
    }
    let mut path = vec![0u16; 512];
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) };
    unsafe { CoTaskMemFree(pidl) };
    if ok == 0 {
        return None;
    }
    let len = path.iter().position(|c| *c == 0).unwrap_or(path.len());
    Some(PathBuf::from(String::from_utf16_lossy(&path[..len])))
}

// ---------- main window ----------

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        WM_CREATE => {
            create_children(hwnd);
            start_scan(crate::to_extended(&PathBuf::from(r"C:\")));
            0
        }
        WM_SIZE => {
            let w = (l & 0xFFFF) as i32;
            let h = ((l >> 16) & 0xFFFF) as i32;
            layout(w, h);
            0
        }
        WM_COMMAND => {
            on_action(hwnd, w & 0xFFFF, ((w >> 16) & 0xFFFF) as u32);
            0
        }
        WM_CONTEXTMENU => {
            // right-click on the largest list
            if w == WNDS.lock().unwrap().list as usize {
                context_menu(hwnd, l);
            }
            0
        }
        WM_TIMER => {
            poll_shared(hwnd);
            0
        }
        WM_PAINT => {
            let mut ps = PaintStruct {
                hdc: std::ptr::null_mut(),
                erase: 0,
                paint: Rect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                restore: 0,
                inc_update: 0,
                reserved: [0; 32],
            };
            unsafe {
                BeginPaint(hwnd, &mut ps);
                EndPaint(hwnd, &ps);
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn layout(w: i32, h: i32) {
    let wnds = WNDS.lock().unwrap();
    // two scrollable areas: tree on top (~58%), largest list below
    let usable = h - 44 - 78;
    let tree_h = usable * 58 / 100;
    let list_y = 44 + tree_h + 6;
    let list_h = (44 + usable) - list_y - 6;
    unsafe {
        MoveWindow(wnds.edit as HWND, 8, 44, w - 16, tree_h, 1);
        MoveWindow(wnds.list as HWND, 8, list_y, w - 16, list_h.max(60), 1);
        MoveWindow(wnds.prog as HWND, 8, h - 62, w - 16, 20, 1);
        MoveWindow(wnds.status as HWND, 10, h - 36, w - 20, 22, 1);
    }
}

fn create_children(hwnd: HWND) {
    let inst = unsafe { GetModuleHandleW(std::ptr::null_mut()) };
    let ui_font = unsafe {
        CreateFontW(
            -13, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, wide("Segoe UI").as_ptr(),
        )
    };
    let mono_font = unsafe {
        CreateFontW(
            -14, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, wide("Consolas").as_ptr(),
        )
    };

    let mk = |class: &str, x: i32, y: i32, w: i32, h: i32, text: &str, style: u32, id: usize| -> HWND {
        unsafe {
            let c = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                style,
                x,
                y,
                w,
                h,
                hwnd,
                id as HMENU,
                inst,
                std::ptr::null_mut(),
            );
            SendMessageW(c, WM_SETFONT, ui_font as usize, 1);
            c
        }
    };
    let btn = |x: i32, w: i32, text: &str, id: usize| {
        mk("BUTTON", x, 8, w, 28, text, WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON, id)
    };

    btn(8, 110, "Scan C:\\", ID_SCAN_C);
    btn(126, 130, "Scan folder...", ID_SCAN_FOLDER);
    btn(264, 120, "Compact WSL", ID_COMPACT);
    mk("STATIC", 392, 12, 40, 20, "View:", WS_CHILD | WS_VISIBLE | SS_LEFT, 0);
    let combo = mk(
        "COMBOBOX",
        434,
        8,
        150,
        200,
        "",
        WS_CHILD | WS_VISIBLE | CBS_DROPDOWNLIST | WS_VSCROLL,
        ID_VIEW,
    );
    for item in ["Biggest files", "Biggest dirs"] {
        unsafe {
            SendMessageW(combo, CB_ADDSTRING, 0, wide(item).as_ptr() as isize);
        }
    }
    unsafe { SendMessageW(combo, CB_SETCURSEL, 0, 0) };
    btn(592, 95, "Explorer", ID_EXPLORER);
    btn(694, 80, "Delete", ID_DELETE);
    // admin relaunch lives in Actions menu; status bar shows [admin] when elevated
    let admin_btn = 0usize as HWND;

    let edit = unsafe {
        let e = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_VSCROLL,
            8,
            44,
            100,
            100,
            hwnd,
            std::ptr::null_mut(),
            inst,
            std::ptr::null_mut(),
        );
        SendMessageW(e, WM_SETFONT, mono_font as usize, 1);
        e
    };
    let list = unsafe {
        let lb = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("LISTBOX").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | LBS_NOTIFY | LBS_DISABLENOSCROLL | LBS_USETABSTOPS | LBS_EXTENDEDSEL | WS_VSCROLL,
            8,
            44,
            100,
            100,
            hwnd,
            ID_LIST as HMENU,
            inst,
            std::ptr::null_mut(),
        );
        SendMessageW(lb, WM_SETFONT, mono_font as usize, 1);
        lb
    };
    let prog = unsafe {
        CreateWindowExW(
            0,
            wide("msctls_progress32").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | PBS_MARQUEE,
            8,
            100,
            100,
            20,
            hwnd,
            std::ptr::null_mut(),
            inst,
            std::ptr::null_mut(),
        )
    };
    unsafe {
        SendMessageW(prog, PBM_SETRANGE32, 0, 1000);
    }
    let status = unsafe {
        let s = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide("ready").as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            10,
            100,
            100,
            22,
            hwnd,
            std::ptr::null_mut(),
            inst,
            std::ptr::null_mut(),
        );
        SendMessageW(s, WM_SETFONT, ui_font as usize, 1);
        s
    };

    {
        let mut g = SHARED.lock().unwrap();
        g.elevated = is_elevated();
    }
    let mut w = WNDS.lock().unwrap();
    w.main = hwnd as usize;
    w.edit = edit as usize;
    w.list = list as usize;
    w.prog = prog as usize;
    w.status = status as usize;
    w.admin_btn = admin_btn as usize;
    w.combo = combo as usize;
}

fn poll_shared(_hwnd: HWND) {
    let mut g = SHARED.lock().unwrap();
    let wnds = WNDS.lock().unwrap();

    if g.busy {
        if let Some(p) = &g.scan_prog {
            g.status = format!(
                "scanning... {} dirs, {} files, {}",
                fmt_num(p.dirs_done.load(std::sync::atomic::Ordering::Relaxed)),
                fmt_num(p.files.load(std::sync::atomic::Ordering::Relaxed)),
                fmt_size(p.bytes.load(std::sync::atomic::Ordering::Relaxed)),
            );
        }
    }

    static LAST: Mutex<String> = Mutex::new(String::new());
    let mut last = LAST.lock().unwrap();
    let mut status = g.status.clone();
    if g.elevated && !status.ends_with("[admin]") {
        status.push_str("   [admin]");
    }
    if *last != status {
        *last = status.clone();
        unsafe {
            SetWindowTextW(wnds.status as HWND, wide(&status).as_ptr());
        }
    }
    drop(last);

    if let Some(f) = g.progress_pct {
        unsafe {
            SendMessageW(wnds.prog as HWND, PBM_SETPOS, (f * 1000.0) as usize, 0);
        }
    }

    if let Some(text) = g.result.take() {
        unsafe {
            SetWindowTextW(wnds.edit as HWND, wide(&text).as_ptr());
            SendMessageW(wnds.prog as HWND, PBM_SETMARQUEE, 0, 0);
            SendMessageW(wnds.prog as HWND, PBM_SETPOS, 1000, 0);
        }
    }
    if let Some(items) = g.list.take() {
        unsafe {
            SendMessageW(wnds.list as HWND, LB_RESETCONTENT, 0, 0);
            for (label, _) in &items {
                SendMessageW(wnds.list as HWND, LB_ADDSTRING, 0, wide(label).as_ptr() as isize);
            }
            SendMessageW(wnds.prog as HWND, PBM_SETMARQUEE, 0, 0);
            SendMessageW(wnds.prog as HWND, PBM_SETPOS, 1000, 0);
        }
    }
    if let Some(msg) = g.done_msg.take() {
        unsafe {
            SendMessageW(wnds.prog as HWND, PBM_SETMARQUEE, 0, 0);
            MessageBoxW(
                wnds.main as HWND,
                wide(&msg).as_ptr(),
                wide("diskhound").as_ptr(),
                MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
            );
        }
    }
}

pub fn run() {
    unsafe {
        SetProcessDPIAware();
        CoInitializeEx(0, 2); // COINIT_APARTMENTTHREADED
        let ic = InitCtrls {
            size: 8,
            icc: 0x0000_40FF,
        };
        InitCommonControlsEx(&ic);

        let inst = GetModuleHandleW(std::ptr::null_mut());
        let class = wide("diskhound_main");

        let bar_menu = CreateMenu();
        let actions = CreateMenu();
        AppendMenuW(actions, MF_STRING, ID_SCAN_C, wide("Scan C:\\").as_ptr());
        AppendMenuW(
            actions,
            MF_STRING,
            ID_SCAN_FOLDER,
            wide("Scan folder...").as_ptr(),
        );
        AppendMenuW(actions, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(actions, MF_STRING, ID_COMPACT, wide("Compact WSL...").as_ptr());
        AppendMenuW(actions, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(
            actions,
            MF_STRING,
            ID_ADMIN,
            wide("Run as administrator").as_ptr(),
        );
        AppendMenuW(actions, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(actions, MF_STRING, ID_EXIT, wide("Exit").as_ptr());
        AppendMenuW(bar_menu, MF_POPUP, actions as usize, wide("Actions").as_ptr());

        let wc = WndClassEx {
            cb_size: std::mem::size_of::<WndClassEx>() as u32,
            style: 3,
            wnd_proc: Some(main_proc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: inst,
            h_icon: std::ptr::null_mut(),
            h_cursor: LoadCursorW(std::ptr::null_mut(), 32512usize as *const u16),
            hbr_background: 16 as HBRUSH,
            menu_name: std::ptr::null(),
            class_name: class.as_ptr(),
            h_icon_sm: std::ptr::null_mut(),
        };
        if RegisterClassExW(&wc) == 0 {
            return;
        }

        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("diskhound").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            -2147483648,
            0,
            1000,
            760,
            std::ptr::null_mut(),
            bar_menu,
            inst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        SetMenu(hwnd, bar_menu);
        SetTimer(hwnd, TIMER_UI, 150, 0);
        ShowWindow(hwnd, 5);
        UpdateWindow(hwnd);
        SetForegroundWindow(hwnd);

        let mut msg = Msg {
            hwnd: std::ptr::null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            pt_x: 0,
            pt_y: 0,
            l_private: 0,
        };
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
