#![cfg(windows)]

mod ffi;
mod state;
mod workers;
mod picker;
mod actions;

pub(crate) use state::is_elevated;

use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::scan;
use crate::{disp, fmt_num, fmt_size};

use actions::*;
use ffi::*;
use state::*;
use workers::*;

// ---------- main window ----------

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        WM_CREATE => {
            create_children(hwnd);
            unsafe {
                PostMessageW(hwnd, WM_APP_LOAD, 0, 0);
            }
            0
        }
        WM_APP_LOAD => {
            let root = crate::to_extended(&PathBuf::from(r"C:\"));
            let main_hwnd = hwnd as usize;
            thread::spawn(move || {
                // show last cached scan instantly, then rescan in background
                if let Some((data, ts)) = scan::load_cache(&cache_path(&root)) {
                    let data = Arc::new(data);
                    let age = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                        .saturating_sub(ts);
                    let admin = {
                        let mut g = SHARED.lock().unwrap();
                        g.data = Some(data.clone());
                        g.status = format!(
                            "{} — {} (cached, {}s old — rescanning...)",
                            disp(&data.root),
                            fmt_size(data.root_total),
                            age
                        );
                        g.elevated
                    };
                    let text = scan::render(&data, 4, 8, 25, 0.2, !admin)
                        .replace('\n', "\r\n");
                    SHARED.lock().unwrap().result = Some(text);
                    refresh_list();
                    unsafe {
                        SendMessageW(main_hwnd as HWND, WM_TIMER, TIMER_UI, 0);
                    }
                }
                start_scan(root);
            });
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
        WM_ERASEBKGND => {
            if DARK.load(Ordering::Relaxed) {
                let mut r = Rect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                unsafe {
                    GetClientRect(hwnd, &mut r);
                    FillRect(w as *mut c_void, &r, dark_brush() as HBRUSH);
                }
                return 1;
            }
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            if DARK.load(Ordering::Relaxed) {
                let hdc = w as *mut c_void;
                unsafe {
                    SetTextColor(hdc, DARK_FG);
                    SetBkColor(hdc, DARK_BG);
                }
                return dark_brush() as isize;
            }
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        WM_CTLCOLORSTATIC => {
            if DARK.load(Ordering::Relaxed) {
                let hdc = w as *mut c_void;
                unsafe {
                    SetTextColor(hdc, DARK_FG);
                    SetBkMode(hdc, 1);
                }
                return dark_brush() as isize;
            }
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
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

const DARK_BG: u32 = 0x001E_1E1E;
const DARK_FG: u32 = 0x00E8_E8E8;

static DARK_BRUSH: Mutex<usize> = Mutex::new(0);

fn dark_brush() -> usize {
    let mut b = DARK_BRUSH.lock().unwrap();
    if *b == 0 {
        *b = unsafe { CreateSolidBrush(DARK_BG) } as usize;
    }
    *b
}

pub(crate) fn apply_theme(hwnd: HWND) {
    let dark = DARK.load(Ordering::Relaxed);
    let (kids, edit) = {
        let w = WNDS.lock().unwrap();
        (w.kids.clone(), w.edit)
    };
    let theme = wide(if dark { "DarkMode_Explorer" } else { "Explorer" });
    unsafe {
        let d: u32 = if dark { 1 } else { 0 };
        DwmSetWindowAttribute(hwnd, 20, &d as *const u32 as *const c_void, 4);
        for k in &kids {
            let h = *k as HWND;
            SetWindowTheme(h, theme.as_ptr(), std::ptr::null());
            InvalidateRect(h, std::ptr::null(), 1);
        }
        SendMessageW(
            edit as HWND,
            EM_SETBKGNDCOLOR,
            0,
            (if dark { DARK_BG } else { 0x00FF_FFFF }) as isize,
        );
        InvalidateRect(hwnd, std::ptr::null(), 1);
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

    let b1 = btn(8, 110, "Scan C:\\", ID_SCAN_C);
    let b2 = btn(126, 130, "Scan folder...", ID_SCAN_FOLDER);
    let b3 = btn(264, 120, "Compact WSL", ID_COMPACT);
    let lbl = mk("STATIC", 392, 12, 40, 20, "View:", WS_CHILD | WS_VISIBLE | SS_LEFT, 0);
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
    let b4 = btn(592, 95, "Explorer", ID_EXPLORER);
    let b5 = btn(694, 80, "Delete", ID_DELETE);
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
    w.kids = vec![
        b1 as usize,
        b2 as usize,
        b3 as usize,
        b4 as usize,
        b5 as usize,
        lbl as usize,
        combo as usize,
        edit as usize,
        list as usize,
        prog as usize,
        status as usize,
    ];
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
            SendMessageW(wnds.prog as HWND, PBM_SETPOS, 0, 0);
        }
    }
    if let Some(items) = g.list.take() {
        unsafe {
            SendMessageW(wnds.list as HWND, LB_RESETCONTENT, 0, 0);
            for (label, _) in &items {
                SendMessageW(wnds.list as HWND, LB_ADDSTRING, 0, wide(label).as_ptr() as isize);
            }
            SendMessageW(wnds.prog as HWND, PBM_SETMARQUEE, 0, 0);
            SendMessageW(wnds.prog as HWND, PBM_SETPOS, 0, 0);
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
        AppendMenuW(actions, MF_STRING, ID_DARK, wide("Dark mode").as_ptr());
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
            1280,
            768,
            std::ptr::null_mut(),
            bar_menu,
            inst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        SetMenu(hwnd, bar_menu);
        // icon: diskhound.ico next to the exe
        let mut exe = vec![0u16; 512];
        let n = GetModuleFileNameW(std::ptr::null_mut(), exe.as_mut_ptr(), 512);
        if n > 0 {
            let p = String::from_utf16_lossy(&exe[..n as usize]);
            if let Some(dir) = std::path::Path::new(&p).parent() {
                let ico = wide(&format!(r"{}\diskhound.ico", dir.display()));
                let big = LoadImageW(std::ptr::null_mut(), ico.as_ptr(), 1, 32, 32, 0x10);
                let small = LoadImageW(std::ptr::null_mut(), ico.as_ptr(), 1, 16, 16, 0x10);
                if !big.is_null() {
                    SendMessageW(hwnd, 0x0080, 1, big as isize);
                }
                if !small.is_null() {
                    SendMessageW(hwnd, 0x0080, 0, small as isize);
                }
            }
        }
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
