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
MATCH = r"C:\Tools\diskhound\sysmon_tray.exe"
EXE = os.path.join(STAGE, "target", "release", "sysmon_tray.exe")
DIAG_WSL = "/mnt/c/Users/therceman/AppData/Local/Temp/sysmon-pawnio.log"


def sync_src():
    ignore = shutil.ignore_patterns("target", ".git", "__pycache__", "node_modules")
    if os.path.exists(STAGE):
        shutil.rmtree(STAGE)
    shutil.copytree(REPO, STAGE, ignore=ignore)


import re

def diag_lines_after(offset):
    try:
        with open(DIAG_WSL, "rb") as f:
            f.seek(offset)
            return f.read().decode(errors="replace").splitlines()
    except OSError:
        return []


def wait_ready(pid, offset, timeout=20):
    """Wait for a VALID thermal sample for the exact pid — 'msr=' with hr=0 ret>=1.
    'ready'/'open hr=0' are progress, not success. Errors surface immediately."""
    tok = re.compile(rf"\bpid={pid}\b")
    deadline = time.time() + timeout
    seen = []
    while time.time() < deadline:
        lines = [l for l in diag_lines_after(offset) if tok.search(l)]
        for l in lines:
            if l not in seen:
                seen.append(l)
        if any("0x80070005" in l or "ACCESS" in l.upper() for l in lines):
            return "access_denied", seen
        if any("missing export" in l or "LoadLibrary failed" in l for l in lines):
            return "init_failed", seen
        if any("cpu_temp_ready" in l for l in lines):
            return "ready", seen
        ok = [l for l in lines if "msr=0x19C" in l and "hr=0x00000000" in l
              and "ret=1" in l and re.search(r"raw=0x8[0-9a-fA-F]{7}", l)]
        if ok:
            return "ready", seen
        time.sleep(0.6)
    return "timeout", seen


def diag_offset():
    try:
        return os.path.getsize(DIAG_WSL)
    except OSError:
        return 0


def launch(elevated):
    ps(f"Start-Process '{DST_WIN}'" + (" -Verb RunAs" if elevated else ""))


def wait_pid(exclude=(), timeout=12):
    deadline = time.time() + timeout
    while time.time() < deadline:
        for pid, cmd in procs("sysmon_tray"):
            if pid not in exclude and cmd.lower().replace('"', "") == MATCH.lower():
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

        before = {p for p, _ in procs("sysmon_tray")}
        for attempt in range(2):
            mark = diag_offset()
            launch(elevated)
            pid = wait_pid(exclude=before)
            if not pid:
                fail("no new sysmon_tray process after launch")
            info(f"UP pid={pid} (elevated={elevated})")
            status, lines = wait_ready(pid, mark)
            info(f"pawnio[{status}]: {' | '.join(lines[-3:]) or 'no diag'}")
            if status == "ready":
                info("CPU temp verified (valid MSR sample)")
                return
            if status == "access_denied":
                if elevated:
                    fail("pawnio open ACCESS_DENIED even elevated")
                info("pawnio needs elevation — relaunching via UAC")
                elevated = True
                kill_matching("sysmon_tray", MATCH)
                if not wait_gone("sysmon_tray", MATCH, timeout=10):
                    fail("old instance still running before elevated relaunch")
                before.add(pid)
                continue
            fail(f"pawnio not ready: {status}")
        fail("pawnio did not become ready")
    finally:
        unlock()


if __name__ == "__main__":
    main()
