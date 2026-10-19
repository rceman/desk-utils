#![cfg(windows)]
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crate::compact::{self, DiskInfo};
use crate::scan::{self, ScanProgress};
use crate::{disp, fmt_num, fmt_size};
use super::ffi::*;
use super::state::*;

// ---------- render dispatch ----------

pub(crate) fn refresh_list() {
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

pub(crate) fn start_scan(root: PathBuf) {
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
        let _ = scan::save_cache(&data, &cache_path(&data.root));
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

pub(crate) fn start_compact(selected: Vec<DiskInfo>) {
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

