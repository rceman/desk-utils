#![windows_subsystem = "windows"]

mod compact;
mod gui;
mod scan;

use std::path::{Path, PathBuf};

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

fn main() {
    #[cfg(windows)]
    gui::run();
    #[cfg(not(windows))]
    eprintln!("diskhound is a Windows-only app");
}
