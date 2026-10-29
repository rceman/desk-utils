"""Shared Windows helpers for deploy scripts (run from WSL)."""
import hashlib, os, re, subprocess, sys, time

PS = ["powershell.exe", "-NoProfile", "-Command"]
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOOLS = "/mnt/c/Tools"

_LOCK = "/tmp/diskhound-deploy.lock"


def ps(cmd, check=True):
    """Run powershell -Command; return stdout. Exit on failure if check."""
    r = subprocess.run(PS + [cmd], capture_output=True, text=True)
    if check and r.returncode != 0:
        sys.exit(f"[deploy] FAILED ({r.returncode}): {cmd}\n{r.stdout}\n{r.stderr}")
    return (r.stdout or "").strip()


def lock():
    """Serialize deploys across concurrent runs."""
    try:
        fd = os.open(_LOCK, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        os.write(fd, str(os.getpid()).encode())
        os.close(fd)
    except FileExistsError:
        sys.exit(f"[deploy] another deploy in progress ({_LOCK}) — remove if stale")


def unlock():
    try:
        os.unlink(_LOCK)
    except FileNotFoundError:
        pass


def wsl_path(win_path):
    drive, rest = win_path.split(":", 1)
    return f"/mnt/{drive.lower()}" + rest.replace("\\", "/")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def procs(name):
    """[(pid, cmdline)] for processes named <name> (with/without .exe)."""
    n = name.rsplit(".", 1)[0] if "." in name else name
    out = ps(f"Get-CimInstance Win32_Process -Filter \"Name='{n}.exe'\" "
             "| Select ProcessId,CommandLine | ConvertTo-Json -Compress", check=False)
    if not out or out == "null":
        return []
    import json
    try:
        rows = json.loads(out)
    except json.JSONDecodeError as e:
        raise RuntimeError(f"process discovery failed for {name}: {e}: {out[:200]}")
    if isinstance(rows, dict):
        rows = [rows]
    return [(int(r["ProcessId"]), r.get("CommandLine") or "") for r in rows]


def kill_matching(name, cmd_substr):
    """Kill only processes whose CommandLine contains cmd_substr."""
    killed = []
    for pid, cmd in procs(name):
        if cmd_substr.lower() in cmd.lower():
            ps(f"Stop-Process -Id {pid} -Force -ErrorAction SilentlyContinue", check=False)
            killed.append(pid)
    return killed


def wait_gone(name, cmd_substr="", timeout=10):
    """Wait until no matching process remains; return True if gone."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        if not [p for p, c in procs(name) if cmd_substr.lower() in c.lower()]:
            return True
        time.sleep(0.4)
    return False


def start(path_or_cmd):
    ps(f"Start-Process '{path_or_cmd}'")


def started_pid(name, cmd_substr="", after_pids=()):
    """Return pid of a matching process not in after_pids, else None."""
    for pid, cmd in procs(name):
        if cmd_substr.lower() in cmd.lower() and pid not in after_pids:
            return pid
    return None


def log_since(path, pos):
    """Read new log content appended after byte offset pos (via WSL path)."""
    try:
        with open(path, "rb") as f:
            f.seek(0, 2)
            if pos > f.tell():
                pos = 0
            f.seek(pos)
            return f.read().decode(errors="replace")
    except OSError:
        return ""


def log_size(path):
    try:
        return os.path.getsize(path)
    except OSError:
        return 0


def info(msg):
    print(f"[deploy] {msg}", flush=True)


def fail(msg):
    sys.exit(f"[deploy] FAIL: {msg}")
