#!/usr/bin/env python3
"""Deploy scrollshot.py -> C:\\Tools\\diskhound\\tools\\scrollshot\\ and restart the app."""
import ast, os, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import REPO, TOOLS, info, kill, log_tail, sh, start, wsl_path

SRC = os.path.join(REPO, "tools", "scrollshot", "scrollshot.py")
DST_WIN = r"C:\Tools\diskhound\tools\scrollshot\scrollshot.py"
DST = wsl_path(DST_WIN)
LOG = r"C:\Users\therceman\Pictures\Screenshots\ScrollShot\scrollshot.log"


def main():
    src = open(SRC, "rb").read()
    ast.parse(src.decode())  # fails fast on syntax errors
    os.makedirs(os.path.dirname(DST), exist_ok=True)
    open(DST, "wb").write(src)
    # verify byte-identical
    if open(DST, "rb").read() != src:
        sys.exit("[deploy] FAIL: deployed file differs from source")
    info("deployed scrollshot.py (verified identical)")

    kill("pythonw")
    time.sleep(0.6)
    sh(f"Start-Process pythonw '{DST_WIN}'")
    time.sleep(2.5)
    tail = log_tail(LOG, 4)
    info("log tail:")
    print(tail)
    if "panel ready" not in tail:
        sys.exit("[deploy] WARN: no 'panel ready' in log — check app")


if __name__ == "__main__":
    main()
