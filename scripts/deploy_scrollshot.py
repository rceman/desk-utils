#!/usr/bin/env python3
"""Deploy scrollshot.py -> C:\\Tools\\diskhound\\tools\\scrollshot\\ and restart the app.

Kills ONLY the pythonw whose CommandLine contains scrollshot.py (never other Python GUIs),
publishes via .new->rename, verifies a FRESH panel-ready log line after restart.
"""
import ast, os, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import (REPO, fail, info, kill_matching, lock, log_since, log_size,
                  procs, ps, sha256, start, unlock, wait_gone, wsl_path)

SRC = os.path.join(REPO, "tools", "scrollshot", "scrollshot.py")
DST_WIN = r"C:\Tools\diskhound\tools\scrollshot\scrollshot.py"
DST = wsl_path(DST_WIN)
LOG_WSL = "/mnt/c/Users/therceman/Pictures/Screenshots/ScrollShot/scrollshot.log"
PYW = r"C:\Program Files\Python312\pythonw.exe"
MATCH = r"diskhound\tools\scrollshot\scrollshot.py"  # full-path fragment (A5)


def main():
    lock()
    try:
        src = open(SRC, "rb").read()
        ast.parse(src.decode())
        os.makedirs(os.path.dirname(DST), exist_ok=True)

        # transactional publish: .new -> rename
        new = DST + ".new"
        open(new, "wb").write(src)
        if sha256(new) != sha256(SRC):
            fail("hash mismatch after staging")
        os.replace(new, DST)
        if open(DST, "rb").read() != src:
            fail("deployed file differs from source")
        info("scrollshot.py published (hash-verified)")

        before = {p for p, _ in procs("pythonw")}
        killed = kill_matching("pythonw", MATCH)
        info(f"killed scrollshot pythonw: {killed or 'none'}")
        if not wait_gone("pythonw", MATCH):
            fail("old scrollshot still running after 10s")

        mark = log_size(LOG_WSL)
        ps(f"Start-Process '{PYW}' '{DST_WIN}'")
        # fresh verification: NEW pid + NEW 'panel ready' after our byte offset
        deadline = time.time() + 15
        while time.time() < deadline:
            pid = next((p for p, c in procs("pythonw")
                        if MATCH.lower() in c.lower() and p not in before), None)
            fresh = log_since(LOG_WSL, mark)
            if pid and "panel ready" in fresh:
                info(f"UP pid={pid}, fresh panel ready OK")
                return
            time.sleep(0.5)
        fail("no fresh 'panel ready' — check " + LOG_WSL)
    finally:
        unlock()


if __name__ == "__main__":
    main()
