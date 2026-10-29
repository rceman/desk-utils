import time, os, threading, re, json, datetime, sys, subprocess
from winpty import PtyProcess

EXE = os.path.expandvars(r"%LOCALAPPDATA%\devin\cli\bin\devin.exe")
HOME = os.path.expanduser("~")
LOG = os.path.join(HOME, "devin_usage.log")

def log(msg):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(f"{datetime.datetime.now():%H:%M:%S} {msg}\n")

p = PtyProcess.spawn([EXE, "--respect-workspace-trust", "false"], dimensions=(45, 140), cwd=HOME)
chunks = []
def reader():
    while True:
        try:
            c = p.read(65536)
        except Exception:
            break
        if not c:
            break
        chunks.append(c)
threading.Thread(target=reader, daemon=True).start()

time.sleep(12)
p.write("/usage"); time.sleep(0.7); p.write("\r"); time.sleep(0.7); p.write("\r")
time.sleep(8)
try:
    p.terminate()
except Exception:
    pass
# guarantee no orphaned CLI process (it keeps keepalive pings alive)
try:
    subprocess.run(["taskkill", "/T", "/F", "/PID", str(p.pid)], capture_output=True, timeout=15)
except Exception:
    pass

txt = re.sub(r'\x1b\[[0-9;?]*[a-zA-Z]', '', "".join(chunks))
m = re.search(r'Weekly.*?(\d+)% used', txt)
reset = re.search(r'resets ([A-Z][a-z]{2} \d+),? (\d+:\d+ [AP]M) \(UTC([+-]\d+)\)', txt)
bal = re.search(r'Extra usage balance\s+\$([\d.]+)', txt)

if not m:
    log("PARSE FAIL")
    sys.exit(1)

out = {"weekly_left": max(0, 100 - int(m.group(1))), "resets_at": 0,
       "balance": float(bal.group(1)) if bal else 0.0}
if reset:
    off = int(reset.group(3))
    dt = datetime.datetime.strptime(f"{reset.group(1)} {datetime.date.today().year} {reset.group(2)}", "%b %d %Y %I:%M %p")
    dt = dt.replace(tzinfo=datetime.timezone(datetime.timedelta(hours=off)))
    out["resets_at"] = int(dt.timestamp())

json.dump(out, open(os.path.join(HOME, ".devin_usage.json"), "w"))
log(f"ok {out}")
