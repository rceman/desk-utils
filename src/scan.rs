use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::{bar, disp, fmt_num, fmt_size};

const TOP_FILES_PER_DIR: usize = 30;

pub struct DirReport {
    pub path: PathBuf,
    pub own_bytes: u64,
    pub files: u64,
    pub subdirs: Vec<PathBuf>,
    pub top_files: Vec<(u64, PathBuf)>,
    pub errors: u64,
}

fn scan_dir(path: &Path) -> DirReport {
    let mut rep = DirReport {
        path: path.to_path_buf(),
        own_bytes: 0,
        files: 0,
        subdirs: Vec::new(),
        top_files: Vec::new(),
        errors: 0,
    };
    let rd = match fs_read_dir(path) {
        Some(r) => r,
        None => {
            rep.errors = 1;
            return rep;
        }
    };
    for e in rd.flatten() {
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => {
                rep.errors += 1;
                continue;
            }
        };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            rep.subdirs.push(e.path());
        } else if ft.is_file() {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            rep.own_bytes += size;
            rep.files += 1;
            if rep.top_files.len() < TOP_FILES_PER_DIR
                || size > rep.top_files.last().map(|f| f.0).unwrap_or(0)
            {
                rep.top_files.push((size, e.path()));
                rep.top_files.sort_by(|a, b| b.0.cmp(&a.0));
                rep.top_files.truncate(TOP_FILES_PER_DIR);
            }
        }
    }
    rep
}

fn fs_read_dir(path: &Path) -> Option<std::fs::ReadDir> {
    std::fs::read_dir(path).ok()
}

#[derive(Default)]
pub struct ScanProgress {
    pub dirs_done: AtomicU64,
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub done: AtomicBool,
}

pub struct ScanData {
    pub root: PathBuf,
    pub dirs: HashMap<PathBuf, DirReport>,
    pub totals: HashMap<PathBuf, u64>,
    pub global_files: Vec<(u64, PathBuf)>,
    pub total_files: u64,
    pub errors: u64,
    pub elapsed: f64,
    pub root_total: u64,
}

pub fn scan(root: PathBuf, progress: Option<Arc<ScanProgress>>) -> ScanData {
    let t0 = Instant::now();
    let (job_tx, job_rx) = channel::<PathBuf>();
    let (rep_tx, rep_rx) = channel::<DirReport>();
    let job_rx = Arc::new(Mutex::new(job_rx));

    let nworkers = thread::available_parallelism()
        .map(|n| (n.get() * 2).clamp(8, 64))
        .unwrap_or(16);

    let mut handles = Vec::with_capacity(nworkers);
    for _ in 0..nworkers {
        let rx = Arc::clone(&job_rx);
        let tx = rep_tx.clone();
        handles.push(thread::spawn(move || loop {
            let job = rx.lock().unwrap().recv();
            match job {
                Ok(p) => {
                    if tx.send(scan_dir(&p)).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }));
    }
    drop(rep_tx);

    job_tx.send(root.clone()).unwrap();
    let mut outstanding = 1usize;
    let mut dirs: HashMap<PathBuf, DirReport> = HashMap::new();
    let mut global_files: Vec<(u64, PathBuf)> = Vec::new();
    let mut total_files = 0u64;
    let mut errors = 0u64;

    while outstanding > 0 {
        let rep = match rep_rx.recv() {
            Ok(r) => r,
            Err(_) => break,
        };
        outstanding -= 1;
        for sd in &rep.subdirs {
            if job_tx.send(sd.clone()).is_ok() {
                outstanding += 1;
            }
        }
        total_files += rep.files;
        errors += rep.errors;
        if let Some(p) = &progress {
            p.dirs_done.fetch_add(1, Ordering::Relaxed);
            p.files.fetch_add(rep.files, Ordering::Relaxed);
            p.bytes.fetch_add(rep.own_bytes, Ordering::Relaxed);
        }
        global_files.extend(rep.top_files.iter().cloned());
        if global_files.len() > 6000 {
            global_files.sort_by(|a, b| b.0.cmp(&a.0));
            global_files.truncate(3000);
        }
        dirs.insert(rep.path.clone(), rep);
    }
    drop(job_tx);
    for h in handles {
        let _ = h.join();
    }
    if let Some(p) = &progress {
        p.done.store(true, Ordering::Relaxed);
    }

    let mut totals: HashMap<PathBuf, u64> = HashMap::with_capacity(dirs.len());
    let mut stack: Vec<(PathBuf, bool)> = vec![(root.clone(), false)];
    while let Some((node, processed)) = stack.pop() {
        if processed {
            let mut t = dirs.get(&node).map(|r| r.own_bytes).unwrap_or(0);
            if let Some(r) = dirs.get(&node) {
                for c in &r.subdirs {
                    t += totals.get(c).copied().unwrap_or(0);
                }
            }
            totals.insert(node, t);
        } else {
            stack.push((node.clone(), true));
            if let Some(r) = dirs.get(&node) {
                for c in &r.subdirs {
                    stack.push((c.clone(), false));
                }
            }
        }
    }

    let root_total = totals.get(&root).copied().unwrap_or(0);
    ScanData {
        root,
        dirs,
        totals,
        global_files,
        total_files,
        errors,
        elapsed: t0.elapsed().as_secs_f64(),
        root_total,
    }
}

fn tree_to(
    out: &mut String,
    data: &ScanData,
    node: &Path,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    top_k: usize,
    min_pct: f64,
) {
    if depth > max_depth {
        return;
    }
    let rep = match data.dirs.get(node) {
        Some(r) => r,
        None => return,
    };
    let mut kids: Vec<(&PathBuf, u64)> = rep
        .subdirs
        .iter()
        .filter_map(|c| data.totals.get(c).map(|t| (c, *t)))
        .collect();
    kids.sort_by(|a, b| b.1.cmp(&a.1));

    let cutoff = data.root_total as f64 * min_pct / 100.0;
    let shown = kids
        .iter()
        .take(top_k)
        .take_while(|(_, s)| *s as f64 >= cutoff)
        .count();
    for (i, (child, size)) in kids.iter().take(shown).enumerate() {
        let last = i == shown - 1 && shown == kids.len();
        let conn = if last { "└─" } else { "├─" };
        let pct = *size as f64 / data.root_total.max(1) as f64 * 100.0;
        let name = child
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| disp(child));
        let _ = writeln!(
            out,
            "{}{} {:>10} {:>5.1}%  {}  {}",
            prefix,
            conn,
            fmt_size(*size),
            pct,
            bar(pct, 20),
            name
        );
        let child_prefix = format!("{}{}  ", prefix, if last { " " } else { "|" });
        tree_to(
            out,
            data,
            child,
            &child_prefix,
            depth + 1,
            max_depth,
            top_k,
            min_pct,
        );
    }
    if kids.len() > shown {
        let rest: u64 = kids.iter().skip(shown).map(|k| k.1).sum();
        let pct = rest as f64 / data.root_total.max(1) as f64 * 100.0;
        let _ = writeln!(
            out,
            "{}└─ {:>10} {:>5.1}%  {}  ... {} more",
            prefix,
            fmt_size(rest),
            pct,
            bar(pct, 20),
            kids.len() - shown
        );
    }
}

pub fn render(
    data: &ScanData,
    max_depth: usize,
    top_k: usize,
    files_n: usize,
    min_pct: f64,
    admin_hint: bool,
) -> String {
    let mut out = String::new();
    let mut head = format!(
        "scanned {} dirs, {} files in {:.1}s ({} skipped: access denied)",
        fmt_num(data.dirs.len() as u64),
        fmt_num(data.total_files),
        data.elapsed,
        fmt_num(data.errors),
    );
    if admin_hint && data.errors > 0 {
        head.push_str(" — run as administrator to include skipped dirs");
    }
    let _ = writeln!(out, "{head}\n");
    let _ = writeln!(out, "{} — {}\n", disp(&data.root), fmt_size(data.root_total));
    tree_to(
        &mut out, data, &data.root, "", 0, max_depth, top_k, min_pct,
    );

    let mut files = data.global_files.clone();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(files_n);
    if !files.is_empty() {
        let _ = writeln!(out, "\nLargest files:");
        for (size, p) in &files {
            let pct = *size as f64 / data.root_total.max(1) as f64 * 100.0;
            let _ = writeln!(out, "  {:>10} {:>5.1}%  {}", fmt_size(*size), pct, disp(p));
        }
    }
    if files.iter().any(|(_, p)| {
        p.extension()
            .map(|e| e.eq_ignore_ascii_case("vhdx"))
            .unwrap_or(false)
    }) {
        let _ = writeln!(
            out,
            "\ntip: Actions > Compact WSL can shrink WSL/VM .vhdx disks and free space"
        );
    }
    out
}

/// Flat "biggest directories" list for the GUI list view: (label, path) sorted desc.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn render_dirs(data: &ScanData, n: usize) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(&PathBuf, u64)> = data
        .totals
        .iter()
        .filter(|(p, _)| **p != data.root)
        .map(|(p, s)| (p, *s))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(n);
    v.iter()
        .map(|(p, s)| {
            let pct = *s as f64 / data.root_total.max(1) as f64 * 100.0;
            (
                format!("{:>10} {:>5.1}%  {}", fmt_size(*s), pct, disp(p)),
                (*p).clone(),
            )
        })
        .collect()
}

/// Flat "biggest files" list for the GUI list view.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn render_files(data: &ScanData, n: usize) -> Vec<(String, PathBuf)> {
    let mut v = data.global_files.clone();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.truncate(n);
    v.iter()
        .map(|(s, p)| {
            let pct = *s as f64 / data.root_total.max(1) as f64 * 100.0;
            (
                format!("{:>10} {:>5.1}%  {}", fmt_size(*s), pct, disp(p)),
                p.clone(),
            )
        })
        .collect()
}

// ---------- persistent scan cache ----------

const CACHE_MAGIC: u32 = 0x4448_4302; // "DHC" + v2 (no per-dir top_files)

fn w_u32(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_le_bytes());
}
fn w_u64(b: &mut Vec<u8>, v: u64) {
    b.extend(v.to_le_bytes());
}
fn w_str(b: &mut Vec<u8>, s: &str) {
    w_u32(b, s.len() as u32);
    b.extend(s.as_bytes());
}
fn w_path(b: &mut Vec<u8>, p: &Path) {
    w_str(b, &p.to_string_lossy());
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}
impl Rd<'_> {
    fn u32(&mut self) -> Option<u32> {
        let e = self.i + 4;
        let v = u32::from_le_bytes(self.b.get(self.i..e)?.try_into().ok()?);
        self.i = e;
        Some(v)
    }
    fn u64(&mut self) -> Option<u64> {
        let e = self.i + 8;
        let v = u64::from_le_bytes(self.b.get(self.i..e)?.try_into().ok()?);
        self.i = e;
        Some(v)
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let e = self.i + n;
        let s = String::from_utf8(self.b.get(self.i..e)?.to_vec()).ok()?;
        self.i = e;
        Some(s)
    }
    fn path(&mut self) -> Option<PathBuf> {
        self.str().map(PathBuf::from)
    }
}

/// Serialize a scan result; returns true on success. Best-effort.
pub fn save_cache(data: &ScanData, path: &Path) -> bool {
    let mut b = Vec::with_capacity(1 << 20);
    w_u32(&mut b, CACHE_MAGIC);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    w_u64(&mut b, ts);
    w_path(&mut b, &data.root);
    w_u64(&mut b, data.elapsed.to_bits());
    w_u64(&mut b, data.root_total);
    w_u64(&mut b, data.total_files);
    w_u64(&mut b, data.errors);
    w_u32(&mut b, data.totals.len() as u32);
    for (p, s) in &data.totals {
        w_path(&mut b, p);
        w_u64(&mut b, *s);
    }
    w_u32(&mut b, data.dirs.len() as u32);
    for (p, r) in &data.dirs {
        w_path(&mut b, p);
        w_u64(&mut b, r.own_bytes);
        w_u64(&mut b, r.files);
        w_u64(&mut b, r.errors);
        w_u32(&mut b, r.subdirs.len() as u32);
        for sd in &r.subdirs {
            w_path(&mut b, sd);
        }
        // per-dir top_files are not cached — render uses global_files only
        w_u32(&mut b, 0);
    }
    w_u32(&mut b, data.global_files.len() as u32);
    for (s, f) in &data.global_files {
        w_u64(&mut b, *s);
        w_path(&mut b, f);
    }
    std::fs::write(path, b).is_ok()
}

/// Load a cached scan; returns (data, unix timestamp of the scan).
pub fn load_cache(path: &Path) -> Option<(ScanData, u64)> {
    let bytes = std::fs::read(path).ok()?;
    let mut r = Rd { b: &bytes, i: 0 };
    if r.u32()? != CACHE_MAGIC {
        return None;
    }
    let ts = r.u64()?;
    let root = r.path()?;
    let elapsed = f64::from_bits(r.u64()?);
    let root_total = r.u64()?;
    let total_files = r.u64()?;
    let errors = r.u64()?;
    let n = r.u32()? as usize;
    let mut totals = HashMap::with_capacity(n);
    for _ in 0..n {
        let p = r.path()?;
        let s = r.u64()?;
        totals.insert(p, s);
    }
    let n = r.u32()? as usize;
    let mut dirs = HashMap::with_capacity(n);
    for _ in 0..n {
        let p = r.path()?;
        let own_bytes = r.u64()?;
        let files = r.u64()?;
        let errors = r.u64()?;
        let ns = r.u32()? as usize;
        let mut subdirs = Vec::with_capacity(ns);
        for _ in 0..ns {
            subdirs.push(r.path()?);
        }
        let nf = r.u32()? as usize;
        let mut top_files = Vec::with_capacity(nf);
        for _ in 0..nf {
            let s = r.u64()?;
            let f = r.path()?;
            top_files.push((s, f));
        }
        dirs.insert(
            p.clone(),
            DirReport {
                path: p,
                own_bytes,
                files,
                subdirs,
                top_files,
                errors,
            },
        );
    }
    let n = r.u32()? as usize;
    let mut global_files = Vec::with_capacity(n);
    for _ in 0..n {
        let s = r.u64()?;
        let f = r.path()?;
        global_files.push((s, f));
    }
    Some((
        ScanData {
            root,
            dirs,
            totals,
            global_files,
            total_files,
            errors,
            elapsed,
            root_total,
        },
        ts,
    ))
}
