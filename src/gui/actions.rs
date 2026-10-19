#![cfg(windows)]
use std::path::PathBuf;

use crate::compact;
use crate::{disp, fmt_size};
use super::ffi::*;
use super::picker::*;
use super::state::*;
use super::workers::*;
use super::apply_theme;
use std::sync::atomic::Ordering;

// ---------- actions ----------

pub(crate) fn selected_paths() -> Vec<PathBuf> {
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

pub(crate) fn open_in_explorer(path: &PathBuf) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", disp(path)))
        .spawn();
}

pub(crate) fn delete_to_recycle(hwnd: HWND, paths: &[PathBuf]) {
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

pub(crate) fn copy_path_to_clipboard(hwnd: HWND, text: &str) {
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

pub(crate) fn relaunch_admin(hwnd: HWND) {
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

pub(crate) fn on_action(hwnd: HWND, id: usize, notify: u32) {
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
        ID_DARK => {
            let d = !DARK.load(Ordering::Relaxed);
            DARK.store(d, Ordering::Relaxed);
            apply_theme(hwnd);
            set_status(if d { "dark mode on" } else { "dark mode off" });
        }
        ID_EXIT => unsafe {
            DestroyWindow(hwnd);
        },
        _ => {}
    }
}

pub(crate) fn context_menu(hwnd: HWND, lparam: isize) {
    // NOTE: drop the WNDS guard immediately — selected_paths() locks it again
    let lb = WNDS.lock().unwrap().list as HWND;
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
        let outside = ((r >> 16) & 0xFFFF) != 0; // HIWORD: point outside list
        if r >= 0 && !outside && idx >= 0 {
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
        DestroyMenu(menu);
        match cmd as usize {
            ID_CTX_EXPLORER => open_in_explorer(&paths[0]),
            ID_CTX_COPYPATH => copy_path_to_clipboard(hwnd, &disp(&paths[0])),
            ID_CTX_DELETE => delete_to_recycle(hwnd, &paths),
            _ => {}
        }
    }
}

pub(crate) fn browse_folder(hwnd: HWND) -> Option<PathBuf> {
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

