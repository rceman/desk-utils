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
    """[(pid, cmdline)] for processes named <name> (with/without .exe).
    Raises RuntimeError on discovery failure — empty result only means truly none."""
    n = name.rsplit(".", 1)[0] if "." in name else name
    r = subprocess.run(PS + [f"Get-CimInstance Win32_Process -Filter \"Name='{n}.exe'\" "
                             "-ErrorAction Stop | Select ProcessId,CommandLine | ConvertTo-Json -Compress"],
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"process discovery failed for {name}: {r.stderr.strip()[:200]}")
    out = (r.stdout or "").strip()
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


def match_cmdline(cmd, exact, interp=""):
    """True if cmd's CommandLine refers to the exact canonical path.
    Quote/case tolerant; never matches other checkouts with the same basename.
    For .py targets the full cmdline must be exactly <interp> <script>."""
    e = exact.lower().replace("/", "\\")
    c = cmd.lower().replace("/", "\\").strip().strip('"')
    if e.endswith(".py"):
        i = interp.lower().replace("/", "\\")
        pat = r'"?' + re.escape(i) + r'"?\s+"?' + re.escape(e) + r'"?'
        return bool(i) and bool(re.fullmatch(pat, c))
    return c == e or c.startswith(e + " ") or c.startswith(e + '"') or (e + '"') in c


def kill_matching(name, exact_path, interp=""):
    """Kill only processes whose CommandLine resolves to exact_path."""
    killed = []
    for pid, cmd in procs(name):
        if match_cmdline(cmd, exact_path, interp):
            ps(f"Stop-Process -Id {pid} -Force -ErrorAction SilentlyContinue", check=False)
            killed.append(pid)
    return killed


def wait_gone(name, exact_path="", timeout=10, interp=""):
    """Wait until no matching process remains; return True if gone."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        if not [p for p, c in procs(name) if match_cmdline(c, exact_path, interp) if exact_path]:
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
