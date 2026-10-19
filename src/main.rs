use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

const TOP_FILES_PER_DIR: usize = 30;

struct DirReport {
    path: PathBuf,
    own_bytes: u64,
    files: u64,
    subdirs: Vec<PathBuf>,
    top_files: Vec<(u64, PathBuf)>,
    errors: u64,
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
    let rd = match fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => {
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
                rep.top_files
                    .sort_by(|a, b| b.0.cmp(&a.0));
                rep.top_files.truncate(TOP_FILES_PER_DIR);
            }
        }
    }
    rep
}

#[cfg(windows)]
fn to_extended(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s.starts_with(r"\\?\") {
        p.to_path_buf()
    } else if s.starts_with(r"\\") {
        PathBuf::from(format!(r"\\?\UNC\{}", &s[2..]))
    } else {
        PathBuf::from(format!(r"\\?\{}", s))
    }
}

#[cfg(not(windows))]
fn to_extended(p: &Path) -> PathBuf {
    p.to_path_buf()
}

fn disp(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    }
}

fn fmt_size(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", b, U[i])
    } else if v >= 100.0 {
        format!("{:.0} {}", v, U[i])
    } else {
        format!("{:.1} {}", v, U[i])
    }
}

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "#".repeat(filled), "-".repeat(width - filled))
}

#[cfg(windows)]
fn set_utf8_console() {
    unsafe extern "system" {
        fn SetConsoleOutputCP(cp: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(65001);
    }
}

#[cfg(not(windows))]
fn set_utf8_console() {}

fn print_tree(
    dirs: &HashMap<PathBuf, DirReport>,
    totals: &HashMap<PathBuf, u64>,
    node: &Path,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    top_k: usize,
    root_total: u64,
    min_pct: f64,
) {
    if depth > max_depth {
        return;
    }
    let rep = match dirs.get(node) {
        Some(r) => r,
        None => return,
    };
    let mut kids: Vec<(&PathBuf, u64)> = rep
        .subdirs
        .iter()
        .filter_map(|c| totals.get(c).map(|t| (c, *t)))
        .collect();
    kids.sort_by(|a, b| b.1.cmp(&a.1));

    let cutoff = root_total as f64 * min_pct / 100.0;
    let shown = kids
        .iter()
        .take(top_k)
        .take_while(|(_, s)| *s as f64 >= cutoff)
        .count();
    for (i, (child, size)) in kids.iter().take(shown).enumerate() {
        let last = i == shown - 1 && shown == kids.len();
        let conn = if last { "└─" } else { "├─" };
        let pct = *size as f64 / root_total.max(1) as f64 * 100.0;
        let name = child
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| disp(child));
        println!(
            "{}{} {:>10} {:>5.1}%  {}  {}",
            prefix,
            conn,
            fmt_size(*size),
            pct,
            bar(pct, 20),
            name
        );
        let child_prefix = format!("{}{}  ", prefix, if last { " " } else { "|" });
        print_tree(
            dirs,
            totals,
            child,
            &child_prefix,
            depth + 1,
            max_depth,
            top_k,
            root_total,
            min_pct,
        );
    }
    if kids.len() > shown {
        let rest: u64 = kids.iter().skip(shown).map(|k| k.1).sum();
        println!(
            "{}└─ {:>10} {:>5.1}%  {}  ... {} more",
            prefix,
            fmt_size(rest),
            rest as f64 / root_total.max(1) as f64 * 100.0,
            bar(rest as f64 / root_total.max(1) as f64 * 100.0, 20),
            kids.len() - shown
        );
    }
}

fn usage() -> ! {
    eprintln!("diskhound — find where disk space went");
    eprintln!("usage: diskhound [PATH] [--depth N] [--top N] [--files N]");
    eprintln!("  PATH    directory to scan (default: C:\\ on Windows, . otherwise)");
    eprintln!("  --depth tree depth shown   (default 4)");
    eprintln!("  --top   children per level (default 8)");
    eprintln!("  --files largest files list (default 25)");
    eprintln!("  --min   min %% of total to show  (default 0.2)");
    std::process::exit(2);
}

fn main() {
    set_utf8_console();

    let mut path_arg: Option<String> = None;
    let mut max_depth = 4usize;
    let mut top_k = 8usize;
    let mut top_files_n = 25usize;
    let mut min_pct = 0.2f64;

    let mut it = env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--depth" => max_depth = it.next().and_then(|v| v.parse().ok()).unwrap_or(max_depth),
            "--top" => top_k = it.next().and_then(|v| v.parse().ok()).unwrap_or(top_k),
            "--files" => {
                top_files_n = it.next().and_then(|v| v.parse().ok()).unwrap_or(top_files_n)
            }
            "--min" => min_pct = it.next().and_then(|v| v.parse().ok()).unwrap_or(min_pct),
            "-h" | "--help" => usage(),
            _ if a.starts_with("--") => usage(),
            _ => path_arg = Some(a),
        }
    }

    let default_root = if cfg!(windows) { r"C:\" } else { "." };
    let root = to_extended(Path::new(path_arg.as_deref().unwrap_or(default_root)));

    println!("diskhound — sniffing out disk usage in {}", disp(&root));
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
        global_files.extend(rep.top_files.iter().cloned());
        if global_files.len() > 4000 {
            global_files.sort_by(|a, b| b.0.cmp(&a.0));
            global_files.truncate(1000);
        }
        dirs.insert(rep.path.clone(), rep);
    }
    drop(job_tx);
    for h in handles {
        let _ = h.join();
    }

    // post-order aggregation of total sizes
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

    let elapsed = t0.elapsed();
    let root_total = totals.get(&root).copied().unwrap_or(0);
    println!(
        "scanned {} dirs, {} files in {:.1}s ({} entries skipped: access denied)\n",
        fmt_num(dirs.len() as u64),
        fmt_num(total_files),
        elapsed.as_secs_f64(),
        fmt_num(errors),
    );
    println!("{} — {}\n", disp(&root), fmt_size(root_total));

    print_tree(
        &dirs,
        &totals,
        &root,
        "",
        0,
        max_depth,
        top_k,
        root_total,
        min_pct,
    );

    global_files.sort_by(|a, b| b.0.cmp(&a.0));
    global_files.truncate(top_files_n);
    if !global_files.is_empty() {
        println!("\nLargest files:");
        for (size, p) in &global_files {
            let pct = *size as f64 / root_total.max(1) as f64 * 100.0;
            println!("  {:>10} {:>5.1}%  {}", fmt_size(*size), pct, disp(p));
        }
    }
    println!();

    if std::io::stdin().is_terminal() {
        use std::io::Write;
        print!("press Enter to exit");
        let _ = std::io::stdout().flush();
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    }
}
