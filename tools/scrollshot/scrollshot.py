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


class Panel:
    def __init__(self, tk, out_dir):
        self.tk = tk
        self.out_dir = out_dir
        self.out_path = None
        self.zone = None
        self.border = None
        self.recording = False
        self.stop_flag = threading.Event()
        self.cancel_flag = threading.Event()
        self.start_flag = threading.Event()
        self.q = queue.Queue()
        self.root = tk.Tk()
        self.root.title("ScrollShot")
        self.root.attributes("-topmost", True)
        self.root.configure(bg="#1e1e1e")
        self.root.geometry("+60+60")
        self.root.resizable(False, False)
        self.status = tk.StringVar(value="Select Area to begin")
        tk.Label(self.root, textvariable=self.status, bg="#1e1e1e", fg="#d0d0d0",
                 font=("Consolas", 10), justify="left", wraplength=360, height=2,
                 anchor="nw").pack(padx=10, pady=(10, 4), anchor="w", fill="x")
        self.info = tk.StringVar(value="")
        tk.Label(self.root, textvariable=self.info, bg="#1e1e1e", fg="#8fbf8f",
                 font=("Consolas", 9), height=1, anchor="w").pack(padx=10, anchor="w", fill="x")
        self.view = tk.Canvas(self.root, bg="#111111", width=360, height=240, highlightthickness=0)
        self.view.pack(padx=10, pady=8)
        global LOG_PATH
        self.folder = out_dir
        os.makedirs(self.folder, exist_ok=True)
        LOG_PATH = os.path.join(self.folder, "scrollshot.log")

        self.border_var = tk.BooleanVar(value=True)
        tk.Checkbutton(self.root, text="Record Area Border", variable=self.border_var,
                       command=self.apply_border, bg="#1e1e1e", fg="#d0d0d0",
                       selectcolor="#333333", activebackground="#1e1e1e",
                       activeforeground="#ffffff",
                       font=("Consolas", 9)).pack(anchor="w", padx=10)
        btns = tk.Frame(self.root, bg="#1e1e1e")
        btns.pack(pady=4, fill="x")
        r0 = tk.Frame(btns, bg="#1e1e1e"); r0.pack()
        tk.Button(r0, text="Select Area", command=self.select_area, bg="#4a2d6a", fg="white",
                  font=("Consolas", 10), width=14).pack(side="left", padx=4, pady=2)
        tk.Button(r0, text="Take screenshot", command=self.shot, bg="#2d5a6a", fg="white",
                  font=("Consolas", 10), width=14).pack(side="left", padx=4, pady=2)
        r1 = tk.Frame(btns, bg="#1e1e1e"); r1.pack()
        self.btn_rec = tk.Button(r1, text="Start", command=self.toggle_rec, bg="#2d6a2d",
                                 fg="white", font=("Consolas", 10), width=14, state="disabled")
        self.btn_rec.pack(side="left", padx=4, pady=2)


        r3 = tk.Frame(btns, bg="#1e1e1e"); r3.pack()
        tk.Button(r3, text="Copy image", command=self.copy_image, bg="#3d3d3d", fg="white",
                  font=("Consolas", 10), width=14).pack(side="left", padx=4, pady=2)
        tk.Button(r3, text="Copy path", command=self.copy_path, bg="#3d3d3d", fg="white",
                  font=("Consolas", 10), width=14).pack(side="left", padx=4, pady=2)
        foot = tk.Frame(self.root, bg="#1e1e1e")
        foot.pack(fill="x", padx=10, pady=(2, 8))
        tk.Label(foot, text=f"Folder: {self.folder}", bg="#1e1e1e", fg="#7aa2c8",
                 font=("Consolas", 8), anchor="w").pack(side="left", fill="x", expand=True)
        tk.Button(foot, text="Open folder", command=self.open_folder, bg="#2d4a6a", fg="white",
                  font=("Consolas", 9), width=11).pack(side="right")
        self.root.protocol("WM_DELETE_WINDOW", self.cancel)
        self.photo = None
        self.worker = threading.Thread(target=self.record_loop, daemon=True)
        log("panel ready")

    def put(self, kind, payload):
        self.q.put((kind, payload))

    def select_area(self):
        zone = select_zone()
        if zone is None:
            return
        self.zone = zone
        self.root.title(f"ScrollShot  {zone[2]}x{zone[3]}")
        self.btn_rec.config(state="normal")
        self.root.geometry(self._place(zone))
        self.apply_border()
        self.status.set("Press Start")
        log(f"zone={zone}")

    def apply_border(self):
        if self.border:
            self.border.close()
            self.border = None
        if self.border_var.get() and self.zone:
            self.border = Border(self.tk, self.zone)

    def _place(self, zone):
        x, y, w, h = zone
        sw, sh = self.root.winfo_screenwidth(), self.root.winfo_screenheight()
        pw, ph = 400, 540
        for cx, cy in [(x + w + 12, y), (x - pw - 12, y), (x, y + h + 12), (x, y - ph - 12),
                       (sw - pw - 12, 12), (12, 12), (sw - pw - 12, sh - ph - 12), (12, sh - ph - 12)]:
            cx, cy = max(0, min(cx, sw - pw)), max(0, min(cy, sh - ph))
            if not intersects(cx, cy, pw, ph, zone):
                return f"{pw}x{ph}+{cx}+{cy}"
        return f"{pw}x{ph}+{max(0, sw - pw - 12)}+12"

    def start(self):
        self.worker.start()
        self.root.after(80, self.poll)
        self.root.mainloop()

    def toggle_rec(self):
        if not self.recording:
            if not self.zone:
                return
            self.stop_flag.clear()
            self.start_flag.set()
            self.recording = True
            self.btn_rec.config(text="Stop & Save", bg="#4a4a2d")
            self.status.set("Recording")
        else:
            self.stop_flag.set()
            self.recording = False
            self.btn_rec.config(text="Start", bg="#2d6a2d")

    def shot(self):
        if not self.zone:
            self.status.set("Select Area first")
            return
        try:
            img = grab(self.zone)
            self.out_path = self._new_path()
            Image.fromarray(img).save(self.out_path)
            self.put("preview", img)
            self.status.set("Screenshot saved")
            self.info.set(f"{img.shape[1]}x{img.shape[0]}")
            log(f"shot saved {self.out_path}")
        except Exception as e:
            log(f"shot error: {e}")
            self.status.set(f"shot error: {e}")

    def _new_path(self):
        return os.path.join(self.folder, f"scrollshot_{datetime.datetime.now():%Y%m%d_%H%M%S}.png")

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
                elif kind == "btn":
                    self.recording = False
                    self.btn_rec.config(text="Start", bg="#2d6a2d", state="normal")
        except queue.Empty:
            pass
        self.root.after(80, self.poll)

    def show(self, arr):
        try:
            img = Image.fromarray(arr)
            w, h = img.size
            s = min(360 / w, 240 / h, 1.0)
            if s < 1.0:
                img = img.resize((int(w * s), int(h * s)), Image.BILINEAR)
            if img.height > 240:
                img = img.crop((0, img.height - 240, img.width, img.height))
            self.photo = ImageTk.PhotoImage(img)
            self.view.delete("all")
            self.view.create_image(180, 120, image=self.photo)
        except Exception as e:
            log(f"show error: {e}")

    def open_folder(self):
        try:
            import subprocess
            if self.out_path and os.path.exists(self.out_path):
                subprocess.Popen(["explorer.exe", f"/select,{self.out_path}"])
            else:
                os.startfile(self.folder)
        except Exception as e:
            log(f"open folder: {e}")

    def copy_image(self):
        try:
            if not (self.out_path and os.path.exists(self.out_path)):
                self.status.set("Nothing to copy yet")
                return
            import subprocess
            ps = ('Add-Type -AssemblyName System.Drawing,System.Windows.Forms;'
                  f'$img=[System.Drawing.Image]::FromFile("{self.out_path}");'
                  '[System.Windows.Forms.Clipboard]::SetImage($img);$img.Dispose()')
            subprocess.Popen(["powershell", "-NoProfile", "-WindowStyle", "Hidden", "-Command", ps],
                             creationflags=0x08000000)
            self.status.set("Image copied to clipboard")
            log(f"copy {self.out_path}")
        except Exception as e:
            log(f"copy error: {e}")

    def copy_path(self):
        try:
            path = self.out_path if (self.out_path and os.path.exists(self.out_path)) else self.folder
            self.root.clipboard_clear()
            self.root.clipboard_append(path)
            self.status.set("Path copied to clipboard")
        except Exception as e:
            log(f"copy path: {e}")

    def cancel(self):
        self.cancel_flag.set()
        self.stop_flag.set()
        if self.border:
            self.border.close()
        self.root.destroy()

    def record_loop(self):
        while not self.cancel_flag.is_set():
            while not self.start_flag.is_set():
                if self.cancel_flag.is_set():
                    return
                time.sleep(0.05)
            zone = self.zone
            chunks, top_chunks = [], []
            canvas = None
            anchor = None
            pos = top = cap_h = 0
            frames = scrolls = 0
            self.put("status", "Recording")
            last_new = time.monotonic()
            try:
                while not self.stop_flag.is_set():
                    t0 = time.monotonic()
                    f = grab(zone)
                    frames += 1
                    fh = f.shape[0]
                    if canvas is None:
                        canvas, anchor, cap_h = f, f, fh
                        self.put("preview", canvas)
                    else:
                        dy, score = find_dy(anchor, f)
                        if dy:
                            anchor = f
                            scrolls += 1
                            last_new = time.monotonic()
                            new_pos = pos + dy
                            if new_pos < top:
                                n = min(top - new_pos, fh)
                                top_chunks.insert(0, f[:n].copy())
                                top, cap_h = new_pos, cap_h + n
                            elif new_pos + fh > top + cap_h:
                                st = top + cap_h - new_pos
                                chunks.append(f[st:].copy())
                                cap_h += fh - st
                            pos = new_pos
                            self.put("preview", canvas if not chunks else np.vstack([canvas, *chunks[-2:]]))
                            self.put("info", f"frames {frames} · scrolls {scrolls} · {cap_h}px")
                    if frames % 5 == 0 and chunks:
                        self.put("preview", np.vstack([canvas[-200:], *chunks[-2:]]))
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
                self.put("info", "failed — nothing saved")
                self.put("btn", "start")
                self.start_flag.clear()
                self.stop_flag.clear()
                continue
            if self.cancel_flag.is_set():
                log("cancelled")
                return
            if canvas is not None:
                if chunks or top_chunks:
                    try:
                        canvas = np.vstack([*top_chunks, canvas, *chunks])
                    except Exception as e:
                        log(f"concat error: {e}")
                if self.cancel_flag.is_set():
                    return
                try:
                    self.out_path = self._new_path()
                    tmp = self.out_path + ".tmp.png"
                    Image.fromarray(canvas).save(tmp)
                    os.replace(tmp, self.out_path)
                    log(f"saved {self.out_path} {canvas.shape}")
                    self.put("preview", canvas)
                    self.put("status", "Screenshot saved — Start for next")
                    self.put("info", f"frames {frames} · scrolls {scrolls} · {canvas.shape[0]}px")
                except Exception as e:
                    log(f"save error: {e}")
                    self.put("status", f"save error: {e}")
            self.start_flag.clear()
            self.stop_flag.clear()
            self.put("btn", "start")


def selftest():
    rng = np.random.default_rng(0)
    tall = rng.random((4000, 400, 3)) * 255
    tall = tall.astype(np.uint8)
    tall[::30] = 30
    h = 400
    for step in (8, 25, 90, 200):
        pos, frames = 0, [tall[:h].copy()]
        for i in range(14):
            pos += step
            frames.append(tall[pos:pos + h].copy())
        canvas, anchor, top, cap_h, pos_d = frames[0], frames[0], 0, h, 0
        for f in frames[1:]:
            dy, sc = find_dy(anchor, f)
            anchor = f
            new_pos = pos_d + dy
            if new_pos < top:
                canvas = np.vstack([f[:min(top - new_pos, h)], canvas])
                top, cap_h = new_pos, cap_h + min(top - new_pos, h)
            elif new_pos + h > top + cap_h:
                st = top + cap_h - new_pos
                canvas = np.vstack([canvas, f[st:]])
                cap_h += h - st
            pos_d = new_pos
        ref = tall[:canvas.shape[0]]
        diff = float(np.abs(canvas.astype(np.int16) - ref.astype(np.int16)).mean())
        print(f"step={step:3d} canvas={canvas.shape[0]} diff={diff:.3f}")
        assert diff == 0, f"step {step} mismatch"
    print("selftest OK")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()
    if args.selftest:
        selftest()
        return
    import tkinter as tk
    out_dir = (os.path.dirname(os.path.abspath(args.out)) if args.out else
               os.path.join(os.path.expanduser("~"), "Pictures", "Screenshots", "ScrollShot"))
    os.makedirs(out_dir, exist_ok=True)
    global LOG_PATH
    LOG_PATH = os.path.join(out_dir, "scrollshot.log")
    log("start")
    try:
        Panel(tk, out_dir).start()
    except Exception:
        import traceback
        log(f"fatal: {traceback.format_exc()}")


if __name__ == "__main__":
    main()
