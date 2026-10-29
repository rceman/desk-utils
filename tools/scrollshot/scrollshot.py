"""ScrollShot: pick a zone (green border), press Start, scroll, get one long PNG."""
import argparse, ctypes, datetime, os, queue, threading, time
import numpy as np
from PIL import Image, ImageTk

# Must run before any window is created: logical px == physical px for mss/grab
try:
    ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
except Exception:
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass

FPS = 15
MAX_DY = 260
MIN_DY = 6
THRESH = 12.0
IDLE_SEC = 10.0
LOG_PATH = None


def log(msg):
    try:
        if LOG_PATH:
            with open(LOG_PATH, "a", encoding="utf-8") as f:
                f.write(f"{datetime.datetime.now():%H:%M:%S.%f}"[:-3] + f" {msg}\n")
    except Exception:
        pass


def grab(zone):
    import mss
    x, y, w, h = zone
    with mss.mss() as sct:
        shot = sct.grab({"left": x, "top": y, "width": w, "height": h})
    return np.array(Image.frombytes("RGB", shot.size, shot.bgra, "raw", "BGRX"))


def find_dy(prev, new):
    """Signed vertical shift: >0 down, <0 up, 0 = no motion."""
    ga = prev.astype(np.float32)
    gb = new.astype(np.float32)
    h = ga.shape[0]
    if h < 100:
        return 0, 999.0
    pa, pb = ga.mean(axis=1), gb.mean(axis=1)
    if float(pa.std()) < 1.5 or float(pb.std()) < 1.5:
        return 0, 999.0
    base = float(np.abs(ga - gb).mean())   # zero-motion pixel error
    lim = min(MAX_DY, h - 60)
    cands = []
    for dy in list(range(1, lim + 1)) + list(range(-1, -lim - 1, -1)):
        if dy > 0:
            a, b = pa[dy:], pb[:h - dy]
        else:
            a, b = pa[:h + dy], pb[-dy:]
        n = min(len(a), len(b))
        if n < 20:
            continue
        cands.append((float(np.abs(a[:n] - b[:n]).mean()), dy))
    if not cands:
        return 0, base
    cands.sort()
    for _, dy in cands[:5]:
        if dy > 0:
            a, b = ga[dy:], gb[:h - dy]
        else:
            a, b = ga[:h + dy], gb[-dy:]
        v = float(np.abs(a - b).mean())
        if v < base * 0.6 and v < THRESH * 2:
            return dy, v
    return 0, base


def select_zone():
    import tkinter as tk
    root = tk.Tk()
    root.attributes("-fullscreen", True)
    root.attributes("-alpha", 0.25)
    root.attributes("-topmost", True)
    root.configure(bg="black")
    cv = tk.Canvas(root, cursor="cross", bg="black", highlightthickness=0)
    cv.pack(fill="both", expand=True)
    st = {"x0": 0, "y0": 0, "rect": None}
    cv.create_text(20, 20, anchor="nw", fill="#00ff88", font=("Consolas", 16),
                   text="Drag to select zone   ·   Esc = cancel")
    cv.bind("<ButtonPress-1>", lambda e: (st.update(x0=e.x_root, y0=e.y_root),
                                          cv.delete(st["rect"]) if st["rect"] else None,
                                          st.update(rect=cv.create_rectangle(e.x_root, e.y_root, e.x_root, e.y_root,
                                                                             outline="#00ff88", width=2))))
    cv.bind("<B1-Motion>", lambda e: st["rect"] and cv.coords(st["rect"], st["x0"], st["y0"], e.x_root, e.y_root))
    def rel(e):
        x, y = min(st["x0"], e.x_root), min(st["y0"], e.y_root)
        w, h = abs(e.x_root - st["x0"]), abs(e.y_root - st["y0"])
        if w > 30 and h > 30:
            st["zone"] = (x, y, w, h)
        root.destroy()
    cv.bind("<ButtonRelease-1>", rel)
    root.bind("<Escape>", lambda e: root.destroy())
    root.mainloop()
    return st.get("zone")


def intersects(px, py, pw, ph, zone):
    x, y, w, h = zone
    return not (px + pw <= x or px >= x + w or py + ph <= y or py >= y + h)


def set_dark_title(root, dark):
    try:
        from ctypes import wintypes
        user32, dwm = ctypes.windll.user32, ctypes.windll.dwmapi
        user32.GetParent.argtypes = [wintypes.HWND]
        user32.GetParent.restype = wintypes.HWND
        dwm.DwmSetWindowAttribute.argtypes = [wintypes.HWND, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD]
        dwm.DwmSetWindowAttribute.restype = ctypes.c_long
        hwnd = user32.GetParent(root.winfo_id()) or root.winfo_id()
        v = ctypes.c_int(1 if dark else 0)
        hr = dwm.DwmSetWindowAttribute(hwnd, 20, ctypes.byref(v), ctypes.sizeof(v))
        log(f"dark title {dark} hwnd={hwnd} hr={hr}")
    except Exception as e:
        log(f"dark title error: {e}")


class Border:
    def __init__(self, tk, zone):
        x, y, w, h = zone
        t = 4
        self.wins = []
        for bx, by, bw, bh in [(x - t, y - t, w + 2 * t, t),
                               (x - t, y + h, w + 2 * t, t),
                               (x - t, y, t, h),
                               (x + w, y, t, h)]:
            win = tk.Toplevel()
            win.overrideredirect(True)
            win.attributes("-topmost", True)
            win.configure(bg="#00ff88")
            win.geometry(f"{bw}x{bh}+{bx}+{by}")
            self.wins.append(win)

    def close(self):
        for w in self.wins:
            try:
                w.destroy()
            except Exception:
                pass
        self.wins = []


class Panel:
    def __init__(self, zone, out_path):
        import tkinter as tk
        self.tk = tk
        self.zone = zone
        self.out_path = out_path
        self.stop_flag = threading.Event()
        self.cancel_flag = threading.Event()
        self.start_flag = threading.Event()
        self.q = queue.Queue()
        self.root = tk.Tk()
        self.root.title("ScrollShot")
        self.root.attributes("-topmost", True)
        self.root.configure(bg="#1e1e1e")
        self.root.geometry(self._place(zone))
        self.root.resizable(False, False)
        self.status = tk.StringVar(value="Press Start to begin")
        tk.Label(self.root, textvariable=self.status, bg="#1e1e1e", fg="#d0d0d0",
                 font=("Consolas", 10), justify="left", wraplength=360, height=2,
                 anchor="nw").pack(padx=10, pady=(10, 4), anchor="w", fill="x")
        self.info = tk.StringVar(value="")
        tk.Label(self.root, textvariable=self.info, bg="#1e1e1e", fg="#8fbf8f",
                 font=("Consolas", 9), height=1, anchor="w").pack(padx=10, anchor="w", fill="x")
        self.view = tk.Canvas(self.root, bg="#111111", width=360, height=260, highlightthickness=0)
        self.view.pack(padx=10, pady=8)
        global LOG_PATH
        self.folder = os.path.dirname(out_path)
        os.makedirs(self.folder, exist_ok=True)
        LOG_PATH = os.path.join(self.folder, "scrollshot.log")
        tk.Label(self.root, text=f"Save to: {self.folder}", bg="#1e1e1e", fg="#7aa2c8",
                 font=("Consolas", 8), wraplength=360, justify="left").pack(padx=10, anchor="w")
        self.dark = tk.BooleanVar(value=True)
        row = tk.Frame(self.root, bg="#1e1e1e")
        row.pack(pady=(6, 0))
        tk.Checkbutton(row, text="Dark title", variable=self.dark, command=self.apply_dark,
                       bg="#1e1e1e", fg="#d0d0d0", selectcolor="#333333",
                       activebackground="#1e1e1e", activeforeground="#ffffff",
                       font=("Consolas", 9)).pack(side="left")
        btns = tk.Frame(self.root, bg="#1e1e1e")
        btns.pack(pady=4, fill="x")
        r1 = tk.Frame(btns, bg="#1e1e1e")
        r1.pack()
        self.btn_start = tk.Button(r1, text="Start", command=self.start_rec, bg="#2d6a2d", fg="white",
                                   font=("Consolas", 10), width=14)
        self.btn_start.pack(side="left", padx=4, pady=2)
        self.btn_stop = tk.Button(r1, text="Stop & Save", command=self.finish, bg="#4a4a2d", fg="white",
                                  font=("Consolas", 10), width=14, state="disabled")
        self.btn_stop.pack(side="left", padx=4, pady=2)
        r2 = tk.Frame(btns, bg="#1e1e1e")
        r2.pack()
        tk.Button(r2, text="Cancel", command=self.cancel, bg="#6a2d2d", fg="white",
                  font=("Consolas", 10), width=14).pack(side="left", padx=4, pady=2)
        tk.Button(r2, text="Open folder", command=self.open_folder, bg="#2d4a6a", fg="white",
                  font=("Consolas", 10), width=14).pack(side="left", padx=4, pady=2)
        r3 = tk.Frame(btns, bg="#1e1e1e")
        r3.pack()
        tk.Button(r3, text="Copy image", command=self.copy_image, bg="#3d3d3d", fg="white",
                  font=("Consolas", 10), width=14).pack(side="left", padx=4, pady=2)
        self.root.protocol("WM_DELETE_WINDOW", self.cancel)
        self.photo = None
        self.worker = threading.Thread(target=self.record_loop, daemon=True)
        self.border = Border(tk, zone)
        log(f"panel {self._place(zone)} zone={zone}")

    @staticmethod
    def _place(zone):
        import tkinter as tk
        x, y, w, h = zone
        r = tk.Tk(); sw, sh = r.winfo_screenwidth(), r.winfo_screenheight(); r.destroy()
        pw, ph = 400, 560
        for cx, cy in [(x + w + 12, y), (x - pw - 12, y), (x, y + h + 12), (x, y - ph - 12),
                       (sw - pw - 12, 12), (12, 12), (sw - pw - 12, sh - ph - 12), (12, sh - ph - 12)]:
            cx, cy = max(0, min(cx, sw - pw)), max(0, min(cy, sh - ph))
            if not intersects(cx, cy, pw, ph, zone):
                return f"{pw}x{ph}+{cx}+{cy}"
        return f"{pw}x{ph}+{max(0, sw - pw - 12)}+12"

    def put(self, kind, payload):
        self.q.put((kind, payload))

    def apply_dark(self):
        set_dark_title(self.root, self.dark.get())

    def start(self):
        self.root.update_idletasks()
        self.root.after(120, self.apply_dark)
        self.worker.start()
        self.root.after(80, self.poll)
        self.root.mainloop()

    def start_rec(self):
        if not self.start_flag.is_set():
            self.start_flag.set()
            self.btn_start.config(state="disabled")
            self.btn_stop.config(state="normal")
            self.status.set("Recording")

    def poll(self):
        try:
            while True:
                kind, payload = self.q.get_nowait()
                if kind == "status":
                    self.status.set(payload)
                elif kind == "info":
                    self.info.set(payload)
                elif kind == "preview":
                    self.show(payload)
        except queue.Empty:
            pass
        except Exception as e:
            log(f"poll error: {e}")
        self.root.after(120, self.poll)

    def show(self, arr):
        try:
            img = Image.fromarray(arr)
            w, h = img.size
            s = min(360 / w, 260 / h, 1.0)
            if s < 1.0:
                img = img.resize((max(1, int(w * s)), max(1, int(h * s))), Image.LANCZOS)
            if img.height > 260:
                img = img.crop((0, img.height - 260, img.width, img.height))
            self.photo = ImageTk.PhotoImage(img)
            self.view.delete("all")
            self.view.create_image(0, 0, anchor="nw", image=self.photo)
        except Exception as e:
            log(f"show error: {e}")

    def open_folder(self):
        try:
            if os.path.exists(self.out_path):
                os.startfile(f'explorer.exe /select,"{self.out_path}"')
            else:
                os.startfile(self.folder)
        except Exception as e:
            log(f"open folder: {e}")

    def copy_image(self):
        try:
            path = self.out_path if os.path.exists(self.out_path) else None
            if not path:
                self.put("status", "Nothing to copy yet")
                return
            ps = ('Add-Type -AssemblyName System.Drawing,System.Windows.Forms;'
                  f'$img=[System.Drawing.Image]::FromFile("{path}");'
                  '[System.Windows.Forms.Clipboard]::SetImage($img);$img.Dispose()')
            import subprocess
            subprocess.Popen(["powershell", "-NoProfile", "-WindowStyle", "Hidden", "-Command", ps],
                             creationflags=0x08000000)
            self.put("status", f"Copied {os.path.basename(path)} to clipboard")
            log(f"copy {path}")
        except Exception as e:
            log(f"copy error: {e}")

    def finish(self):
        self.stop_flag.set()

    def cancel(self):
        self.cancel_flag.set()
        self.stop_flag.set()
        self.border.close()
        self.root.after(200, self.root.destroy)

    def record_loop(self):
        chunks = []          # bottom-appended rows
        top_chunks = []      # prepended rows (scroll above start)
        canvas = None
        anchor = None        # last frame with accepted motion
        pos = 0              # doc position of anchor top
        top = 0              # doc top of captured region
        cap_h = 0            # doc height of captured region
        frames = 0
        while not self.start_flag.is_set():
            if self.stop_flag.is_set() or self.cancel_flag.is_set():
                return
            time.sleep(0.05)
        self.put("status", "Recording")
        last_new = time.monotonic()
        try:
            while not self.stop_flag.is_set():
                t0 = time.monotonic()
                f = grab(self.zone)
                frames += 1
                fh = f.shape[0]
                if canvas is None:
                    canvas = f
                    anchor = f
                    cap_h = fh
                    self.put("preview", canvas)
                    self.put("info", f"zone {self.zone[2]}x{self.zone[3]}  recording…")
                else:
                    dy, score = find_dy(anchor, f)
                    log(f"frame {frames}: dy={dy} score={score:.1f} pos={pos}")
                    if dy:
                        anchor = f
                        last_new = time.monotonic()
                        new_pos = pos + dy
                        if new_pos < top:
                            n = min(top - new_pos, fh)
                            top_chunks.insert(0, f[:n].copy())
                            top, cap_h = new_pos, cap_h + n
                            log(f"prepend {n} rows top={top}")
                        elif new_pos + fh > top + cap_h:
                            start_r = top + cap_h - new_pos
                            chunks.append(f[start_r:].copy())
                            cap_h += fh - start_r
                            log(f"append {fh - start_r} rows cap_h={cap_h}")
                        else:
                            log("frame covered")
                        pos = new_pos
                        total = cap_h
                        self.put("preview", canvas if not chunks else np.vstack([canvas, *chunks[-2:]]))
                        self.put("info", f"frames: {frames}  dy: {dy}  total {total}px")
                left = IDLE_SEC - (time.monotonic() - last_new)
                self.put("status", f"Recording · auto-save in {max(0, int(left + 0.999))}s")
                if left <= 0:
                    log("idle stop")
                    break
                dt = 1.0 / FPS - (time.monotonic() - t0)
                if dt > 0:
                    time.sleep(dt)
        except Exception as e:
            import traceback
            log(f"record error: {e}\n{traceback.format_exc()}")
            self.put("status", f"error: {e}")
            self.put("info", "recording failed — nothing saved")
            self.border.close()
            return
        if self.cancel_flag.is_set():
            log("cancelled")
            return
        if canvas is None:
            return
        if chunks or top_chunks:
            try:
                canvas = np.vstack([*top_chunks, canvas, *chunks])
            except Exception as e:
                log(f"final concat error: {e}")
                self.put("status", f"save error: {e}")
                return
        if self.cancel_flag.is_set():
            log("cancelled before save")
            return
        try:
            tmp = self.out_path + ".tmp.png"
            Image.fromarray(canvas).save(tmp)
            if self.cancel_flag.is_set():
                os.remove(tmp)
                log("cancelled during save")
                return
            os.replace(tmp, self.out_path)
            log(f"saved {self.out_path} {canvas.shape}")
            self.put("preview", canvas)
            self.put("status", f"Saved {os.path.basename(self.out_path)}  {canvas.shape[1]}x{canvas.shape[0]}")
            self.put("info", f"frames: {frames}  → {self.out_path}")
        except Exception as e:
            import traceback
            log(f"save error: {e}\n{traceback.format_exc()}")
            self.put("status", f"save error: {e}")


def selftest():
    rng = np.random.default_rng(0)
    tall = (rng.random((3000, 300, 3)) * 100 + 80).astype(np.uint8)
    tall[::22] = 25
    fh = 600
    for step in (8, 25, 90, 200):
        canvas, prev, pos = None, None, 0
        while pos + fh <= tall.shape[0] - step * 2:
            f = tall[pos:pos + fh]
            if canvas is None:
                canvas = f.copy()
            else:
                dy, sc = find_dy(prev, f)
                assert dy == step, f"step {step}: dy={dy} score={sc:.1f}"
                canvas = np.vstack([canvas, f[fh - dy:]])
            prev = f
            pos += step
        ref = tall[:canvas.shape[0]]
        diff = float(np.abs(canvas.astype(np.int16) - ref.astype(np.int16)).mean())
        print(f"step={step:3d} canvas={canvas.shape[0]:5d} diff={diff:.3f}")
        assert diff < 0.5
    print("selftest OK")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--out", default=None)
    a = ap.parse_args()
    if a.selftest:
        selftest()
        return
    zone = select_zone()
    if not zone:
        return
    out = a.out or os.path.join(os.path.expanduser("~"), "Pictures", "Screenshots", "ScrollShot",
                                f"scrollshot_{datetime.datetime.now():%Y%m%d_%H%M%S}.png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Panel(zone, out).start()


if __name__ == "__main__":
    main()
