#!/usr/bin/env python3
"""Build + deploy sysmon_tray.exe (isolated staging, checked cargo, PID/log verify).

CPU temp requires elevation (PawnIO open -> 0x80070005 otherwise). Pass --elevated
to relaunch via UAC after publish and verify 'ready'/'msr=' in the diag log.
"""
import os, shutil, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import (REPO, fail, info, kill_matching, lock, procs, ps,
                  sha256, start, unlock, wait_gone, wsl_path)

STAGE_WIN = r"C:\Tools\dh-build-sysmon"
STAGE = wsl_path(STAGE_WIN)
DST_WIN = r"C:\Tools\diskhound\sysmon_tray.exe"
DST = wsl_path(DST_WIN)
MATCH = r"diskhound\sysmon_tray.exe"   # full-path fragment, not basename (A5)
EXE = os.path.join(STAGE, "target", "release", "sysmon_tray.exe")
DIAG_WSL = "/mnt/c/Users/therceman/AppData/Local/Temp/sysmon-pawnio.log"


def sync_src():
    ignore = shutil.ignore_patterns("target", ".git", "__pycache__", "node_modules")
    if os.path.exists(STAGE):
        shutil.rmtree(STAGE)
    shutil.copytree(REPO, STAGE, ignore=ignore)


def diag_for(pid, timeout=12):
    """Last pawnio diag lines for <pid>, waiting up to <timeout>s for any."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            tail = open(DIAG_WSL).read()
        except OSError:
            tail = ""
        lines = [l for l in tail.strip().splitlines() if f"pid={pid}" in l]
        if lines:
            return lines
        time.sleep(0.5)
    return []


def launch(elevated):
    ps(f"Start-Process '{DST_WIN}'" + (" -Verb RunAs" if elevated else ""))


def wait_pid(timeout=12):
    deadline = time.time() + timeout
    while time.time() < deadline:
        for pid, cmd in procs("sysmon_tray"):
            if MATCH.lower() in cmd.lower():
                return pid
        time.sleep(0.5)
    return None


def main():
    elevated = "--elevated" in sys.argv
    lock()
    try:
        sync_src()
        out = ps(f"Set-Location '{STAGE_WIN}'; cargo build --release --bin sysmon_tray "
                 "2>&1 | Select-Object -Last 6; exit $LASTEXITCODE")
        print(out)
        if not os.path.exists(EXE):
            fail("cargo did not produce sysmon_tray.exe")

        killed = kill_matching("sysmon_tray", MATCH)
        info(f"killed: {killed or 'none'}")
        if not wait_gone("sysmon_tray", MATCH, timeout=15):
            fail("sysmon_tray still running (elevated instance? kill via Task Manager)")

        tmp = DST + ".new"
        shutil.copy2(EXE, tmp)
        if sha256(tmp) != sha256(EXE):
            fail("hash mismatch after staging")
        os.replace(tmp, DST)
        info(f"published -> {DST_WIN} (sha {sha256(DST)[:12]})")

        for attempt in range(2):
            launch(elevated)
            pid = wait_pid()
            if not pid:
                fail("no new sysmon_tray process after launch")
            info(f"UP pid={pid} (elevated={elevated})")
            lines = diag_for(pid)
            joined = " | ".join(lines[-3:]) if lines else "no diag"
            info(f"pawnio: {joined}")
            if any("ready" in l or "msr=" in l for l in lines):
                info("pawnio ready — CPU temp verified")
                return
            if any("0x80070005" in l or "ACCESS" in l.upper() for l in lines):
                if elevated:
                    fail("pawnio open ACCESS_DENIED even elevated")
                info("pawnio needs elevation — relaunching via UAC")
                elevated = True
                kill_matching("sysmon_tray", MATCH)
                wait_gone("sysmon_tray", MATCH, timeout=10)
                continue
            # exports/driver failed for another reason — surface it, don't claim OK
            fail(f"pawnio not ready: {joined}")
        fail("pawnio did not become ready")
    finally:
        unlock()


if __name__ == "__main__":
    main()
