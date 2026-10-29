"""Shared Windows helpers for deploy scripts (run from WSL)."""
import os, subprocess, sys

PS = ["powershell.exe", "-NoProfile", "-Command"]
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOOLS = "/mnt/c/Tools"


def sh(cmd, check=False):
    """Run a powershell command, return stripped stdout."""
    r = subprocess.run(PS + [cmd], capture_output=True, text=True)
    if check and r.returncode != 0:
        sys.exit(f"FAILED: {cmd}\n{r.stdout}\n{r.stderr}")
    return (r.stdout or "").strip()


def wsl_path(win_path):
    """C:\\Tools\\x -> /mnt/c/Tools/x"""
    drive, rest = win_path.split(":", 1)
    return f"/mnt/{drive.lower()}" + rest.replace("\\", "/")


def kill(proc):
    sh(f"Get-Process {proc} -ErrorAction SilentlyContinue | Stop-Process -Force")


def start(path):
    sh(f"Start-Process '{path}'")


def log_tail(path, n=5):
    return sh(f"Get-Content '{path}' -Tail {n} -ErrorAction SilentlyContinue")


def info(msg):
    print(f"[deploy] {msg}", flush=True)
