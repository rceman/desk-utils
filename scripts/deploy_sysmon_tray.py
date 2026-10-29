#!/usr/bin/env python3
"""Build + deploy sysmon_tray.exe (cargo on Windows-local copy, no UNC paths)."""
import os, shutil, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import REPO, TOOLS, info, kill, sh, start

BUILD_SRC = os.path.join(TOOLS, "dh-src")          # /mnt/c/Tools/dh-src
BUILD_SRC_WIN = r"C:\Tools\dh-src"
EXE = os.path.join(BUILD_SRC, "target", "release", "sysmon_tray.exe")
DST_WIN = r"C:\Tools\diskhound\sysmon_tray.exe"


def sync_src():
    """Copy repo (minus heavy dirs) to the Windows build dir."""
    ignore = shutil.ignore_patterns("target", ".git", "__pycache__", "node_modules")
    if os.path.exists(BUILD_SRC):
        shutil.rmtree(BUILD_SRC)
    shutil.copytree(REPO, BUILD_SRC, ignore=ignore)
    info(f"synced repo -> {BUILD_SRC}")


def main():
    sync_src()
    out = sh(f"Set-Location '{BUILD_SRC_WIN}'; cargo build --release --bin sysmon_tray 2>&1 | Select-Object -Last 4")
    print(out)
    if "Finished" not in out or not os.path.exists(EXE):
        sys.exit("[deploy] FAIL: build did not produce sysmon_tray.exe")

    kill("sysmon_tray")
    sh(f"Copy-Item '{os.path.join(BUILD_SRC_WIN, r'target\release\sysmon_tray.exe')}' '{DST_WIN}' -Force", check=True)
    info(f"deployed -> {DST_WIN}")
    start(DST_WIN)
    info("restarted")


if __name__ == "__main__":
    main()
