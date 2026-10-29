#!/usr/bin/env python3
"""Build + deploy sysmon_tray.exe (isolated staging, checked cargo, PID/log verify)."""
import os, shutil, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import (REPO, TOOLS, fail, info, kill_matching, lock, procs, ps,
                  sha256, start, unlock, wait_gone, wsl_path)

STAGE_WIN = r"C:\Tools\dh-build-sysmon"
STAGE = wsl_path(STAGE_WIN)
DST_WIN = r"C:\Tools\diskhound\sysmon_tray.exe"
DST = wsl_path(DST_WIN)
EXE = os.path.join(STAGE, "target", "release", "sysmon_tray.exe")
DIAG_WSL = "/mnt/c/Users/therceman/AppData/Local/Temp/sysmon-pawnio.log"


def sync_src():
    ignore = shutil.ignore_patterns("target", ".git", "__pycache__", "node_modules")
    if os.path.exists(STAGE):
        shutil.rmtree(STAGE)
    shutil.copytree(REPO, STAGE, ignore=ignore)


def main():
    lock()
    try:
        sync_src()
        out = ps(f"Set-Location '{STAGE_WIN}'; cargo build --release --bin sysmon_tray "
                 "2>&1 | Select-Object -Last 6; exit $LASTEXITCODE")
        print(out)
        if not os.path.exists(EXE):
            fail("cargo did not produce sysmon_tray.exe")

        killed = kill_matching("sysmon_tray", "sysmon_tray.exe")
        info(f"killed: {killed or 'none'}")
        if not wait_gone("sysmon_tray", timeout=15):
            fail("sysmon_tray still running (elevated instance? kill via Task Manager)")

        tmp = DST + ".new"
        shutil.copy2(EXE, tmp)
        if sha256(tmp) != sha256(EXE):
            fail("hash mismatch after staging")
        os.replace(tmp, DST)
        info(f"published -> {DST_WIN} (sha {sha256(DST)[:12]})")

        start(DST_WIN)
        time.sleep(4)
        pid = next((p for p, c in procs("sysmon_tray") if DST_WIN.lower() in c.lower()), None)
        if not pid:
            # elevated sibling may hold the mutex -> tray exits; report diagnostics
            tail = open(DIAG_WSL).read()[-400:] if os.path.exists(DIAG_WSL) else ""
            fail(f"no new sysmon_tray process; last diag:\n{tail}")
        info(f"UP pid={pid}")

        tail = open(DIAG_WSL).read() if os.path.exists(DIAG_WSL) else ""
        last = [l for l in tail.strip().splitlines() if f"pid={pid}" in l][-1:] or ["no pawnio diag yet"]
        info(f"pawnio: {last[0]}")
    finally:
        unlock()


if __name__ == "__main__":
    main()
