#![cfg(windows)]
use crate::compact::DiskInfo;
use crate::{disp, fmt_size};
use super::ffi::*;
use super::state::*;

// ---------- disk picker ----------

pub(crate) fn pick_disks(parent: HWND, disks: &[DiskInfo]) -> Option<Vec<DiskInfo>> {
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

