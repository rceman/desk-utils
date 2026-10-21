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
        let prog_hwnd = WNDS.lock().unwrap().prog as HWND;
        ShowWindow(prog_hwnd, 5);
        SendMessageW(prog_hwnd, PBM_SETMARQUEE, 1, 30);
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
    unsafe {
        let prog_hwnd = WNDS.lock().unwrap().prog as HWND;
        ShowWindow(prog_hwnd, 5);
        SendMessageW(prog_hwnd, PBM_SETRANGE32, 0, 1000);
    }
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


/// Subtract deleted paths from cached scan data — instant UI update,
/// no rescan. Sizes become approximate for dir deletion (descendants
/// under removed dirs are dropped from the index too).
pub(crate) fn apply_deletion(paths: &[PathBuf]) {
    let (arc, main_hwnd, admin_hint) = {
        let g = SHARED.lock().unwrap();
        let Some(a) = g.data.clone() else { return };
        (a, WNDS.lock().unwrap().main, !g.elevated)
    };
    let paths = paths.to_vec();
    thread::spawn(move || {
        let mut d = (*arc).clone();
        for p in &paths {
            let size = d.totals.get(p).copied().unwrap_or_else(|| {
                d.global_files
                    .iter()
                    .find(|(_, f)| f == p)
                    .map(|(s, _)| *s)
                    .unwrap_or(0)
            });
            for anc in p.ancestors() {
                if let Some(t) = d.totals.get_mut(anc) {
                    *t = t.saturating_sub(size);
                }
            }
            if let Some(parent) = p.parent() {
                if let Some(rep) = d.dirs.get_mut(parent) {
                    rep.subdirs.retain(|sd| sd != p);
                }
            }
            let removed_files = d.dirs.get(p).map(|r| r.files).unwrap_or(1);
            d.total_files = d.total_files.saturating_sub(removed_files);
            d.root_total = d.root_total.saturating_sub(size);
            // drop the entry and everything underneath it
            d.totals.retain(|k, _| !k.starts_with(p));
            d.dirs.retain(|k, _| !k.starts_with(p));
            d.global_files.retain(|(_, f)| !f.starts_with(p));
        }
        let data = Arc::new(d);
        let _ = scan::save_cache(&data, &cache_path(&data.root));
        let tree = scan::render(&data, 4, 8, 25, 0.2, admin_hint).replace('\n', "\r\n");
        {
            let mut g = SHARED.lock().unwrap();
            g.data = Some(data.clone());
            g.result = Some(tree);
            g.status = format!(
                "{} — {} (index updated after delete)",
                disp(&data.root),
                fmt_size(data.root_total)
            );
        }
        refresh_list();
        unsafe {
            SendMessageW(main_hwnd as HWND, WM_TIMER, TIMER_UI, 0);
        }
    });
}
