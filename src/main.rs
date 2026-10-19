#![windows_subsystem = "windows"]

mod compact;
mod gui;
mod scan;

use std::env;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

#[cfg(windows)]
fn attach_console() {
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    unsafe {
        AttachConsole(0xFFFF_FFFF); // ATTACH_PARENT_PROCESS
    }
}

#[cfg(not(windows))]
fn attach_console() {}

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

fn usage() -> ! {
    eprintln!("diskhound — find where disk space went");
    eprintln!("usage: diskhound [PATH] [--depth N] [--top N] [--files N] [--min PCT]");
    eprintln!("       diskhound compact [FILE.vhdx] [--yes] [--dry-run] [--no-shutdown]");
    eprintln!("       diskhound              (no args) -> GUI on Windows");
    eprintln!("  PATH    directory to scan (default: C:\\ on Windows, . otherwise)");
    eprintln!("  --depth tree depth shown   (default 4)");
    eprintln!("  --top   children per level (default 8)");
    eprintln!("  --files largest files list (default 25)");
    eprintln!("  --min   min %% of total to show  (default 0.2)");
    std::process::exit(2);
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();

    #[cfg(windows)]
    if argv.is_empty() || argv.iter().any(|a| a == "--gui") {
        gui::run();
        return;
    }

    attach_console();
    set_utf8_console();

    if argv.first().map(|a| a == "compact").unwrap_or(false) {
        let mut path_arg = None;
        let mut yes = false;
        let mut dry = false;
        let mut no_shutdown = false;
        for a in &argv[1..] {
            match a.as_str() {
                "--yes" | "-y" => yes = true,
                "--dry-run" | "-n" => dry = true,
                "--no-shutdown" => no_shutdown = true,
                "-h" | "--help" => usage(),
                _ if a.starts_with("--") => usage(),
                _ => path_arg = Some(a.clone()),
            }
        }
        std::process::exit(compact::run(path_arg, yes, dry, no_shutdown));
    }

    let mut path_arg: Option<String> = None;
    let mut max_depth = 4usize;
    let mut top_k = 8usize;
    let mut top_files_n = 25usize;
    let mut min_pct = 0.2f64;

    let mut it = argv.into_iter();
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
    let data = scan::scan(root, None);
    #[cfg(windows)]
    let admin_hint = !gui::is_elevated();
    #[cfg(not(windows))]
    let admin_hint = false;
    let text = scan::render(&data, max_depth, top_k, top_files_n, min_pct, admin_hint);
    println!("{text}");

    if std::io::stdin().is_terminal() {
        use std::io::Write;
        print!("press Enter to exit");
        let _ = std::io::stdout().flush();
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    }
}
