"""ScrollShot: select a zone, scroll it, get one long PNG.

Capture model: continuous recording (~15 fps) while you scroll. Consecutive
frames differ by a small dy, so alignment is unambiguous; new rows are appended
to the canvas online (no giant frame buffers, no guessing across big jumps).
"""
import argparse, datetime, os, threading, time
import numpy as np
from PIL import Image, ImageTk

FPS = 15
MAX_DY = 260          # max px shift between two consecutive frames
MIN_DY = 6            # smaller shifts are treated as "no scroll"
THRESH = 12.0         # mean-abs profile error accepted as a match
IDLE_SEC = 10.0        # auto-stop after this long without new content
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


def profiles(img):
    g = img.mean(axis=2)
    return g.mean(axis=1), g


def find_dy(prev, new):
    """Vertical shift of document content between two consecutive frames."""
    pa, ga = profiles(prev)
    pb, gb = profiles(new)
    h = len(pa)
    hi = min(MAX_DY, h - 60)
    best_dy, best = 0, float("inf")
    for dy in range(MIN_DY, hi):
        score = float(np.abs(pa[dy:] - pb[:h - dy]).mean())
        if score < best:
            best, best_dy = score, dy
    if best_dy and best < THRESH:
        # verify on real pixels (sampled rows, full width)
        a = ga[best_dy:h:7].astype(np.int16)
        b = gb[:h - best_dy:7].astype(np.int16)
        n = min(len(a), len(b))
        v = float(np.abs(a[:n] - b[:n]).mean())
        if v < THRESH:
            return best_dy, v
    return 0, best


# ---------- zone selector ----------

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

    def press(e):
        st["x0"], st["y0"] = e.x_root, e.y_root
        if st["rect"]:
            cv.delete(st["rect"])
        st["rect"] = cv.create_rectangle(e.x_root, e.y_root, e.x_root, e.y_root, outline="#00ff88", width=2)

    def drag(e):
        if st["rect"]:
            cv.coords(st["rect"], st["x0"], st["y0"], e.x_root, e.y_root)

    def release(e):
        x1, y1 = e.x_root, e.y_root
        x, y = min(st["x0"], x1), min(st["y0"], y1)
        w, h = abs(x1 - st["x0"]), abs(y1 - st["y0"])
        if w > 30 and h > 30:
            st["zone"] = (x, y, w, h)
            root.destroy()

    cv.bind("<ButtonPress-1>", press)
    cv.bind("<B1-Motion>", drag)
    cv.bind("<ButtonRelease-1>", release)
    root.bind("<Escape>", lambda e: root.destroy())
    root.mainloop()
    return st.get("zone")


def intersects(px, py, pw, ph, zone):
    x, y, w, h = zone
    return not (px + pw <= x or px >= x + w or py + ph <= y or py >= y + h)


class Panel:
    def __init__(self, zone, out_path):
        import tkinter as tk
        self.zone = zone
        self.out_path = out_path
        self.stop_flag = threading.Event()
        self.cancel_flag = threading.Event()
        self.canvas_img = None
        self.root = tk.Tk()
        self.root.title("ScrollShot")
        self.root.attributes("-topmost", True)
        self.root.configure(bg="#1e1e1e")
        self.root.geometry(self._place(zone))
        self.status = tk.StringVar(value="Recording — scroll the zone")
        tk.Label(self.root, textvariable=self.status, bg="#1e1e1e", fg="#d0d0d0",
                 font=("Consolas", 10), justify="left", wraplength=340).pack(padx=10, pady=(10, 4), anchor="w")
        self.info = tk.StringVar(value="waiting for first frame…")
        tk.Label(self.root, textvariable=self.info, bg="#1e1e1e", fg="#8fbf8f",
                 font=("Consolas", 9)).pack(padx=10, anchor="w")
        self.view = tk.Canvas(self.root, bg="#111111", width=330, height=250, highlightthickness=0)
        self.view.pack(padx=10, pady=8)
        global LOG_PATH
        self.folder = os.path.dirname(out_path)
        os.makedirs(self.folder, exist_ok=True)
        LOG_PATH = os.path.join(self.folder, "scrollshot.log")
        tk.Label(self.root, text=f"Save to: {self.folder}", bg="#1e1e1e", fg="#7aa2c8",
                 font=("Consolas", 8), wraplength=340, justify="left").pack(padx=10, anchor="w")
        btns = tk.Frame(self.root, bg="#1e1e1e")
        btns.pack(pady=4)
        tk.Button(btns, text="Stop & Save", command=self.finish, bg="#2d6a2d", fg="white",
                  font=("Consolas", 10), width=12).pack(side="left", padx=4)
        tk.Button(btns, text="Cancel", command=self.cancel, bg="#6a2d2d", fg="white",
                  font=("Consolas", 10), width=8).pack(side="left", padx=4)
        tk.Button(btns, text="Open folder", command=self.open_folder, bg="#2d4a6a", fg="white",
                  font=("Consolas", 10), width=11).pack(side="left", padx=4)
        self.root.protocol("WM_DELETE_WINDOW", self.cancel)
        self.photo = None
        self.worker = threading.Thread(target=self.record_loop, daemon=True)
        log(f"panel {self.root.geometry()} zone={zone}")

    @staticmethod
    def _place(zone):
        import tkinter as tk
        x, y, w, h = zone
        r = tk.Tk(); sw, sh = r.winfo_screenwidth(), r.winfo_screenheight(); r.destroy()
        pw, ph = 380, 470
        for cx, cy in [(x + w + 10, y), (x - pw - 10, y), (x, y + h + 10), (x, y - ph - 10),
                       (sw - pw - 10, 10), (10, 10), (sw - pw - 10, sh - ph - 10), (10, sh - ph - 10)]:
            cx, cy = max(0, min(cx, sw - pw)), max(0, min(cy, sh - ph))
            if not intersects(cx, cy, pw, ph, zone):
                return f"{pw}x{ph}+{cx}+{cy}"
        return f"{pw}x{ph}+{max(0, sw - pw - 10)}+10"

    def start(self):
        self.worker.start()
        self.root.after(80, self.poll)
        self.root.mainloop()

    def poll(self):
        try:
            if self.canvas_img is not None:
                self.show(self.canvas_img)
                self.canvas_img = None
        except Exception as e:
            log(f"poll error: {e}")
        self.root.after(150, self.poll)

    def show(self, arr):
        try:
            img = Image.fromarray(arr)
            w, h = img.size
            s = min(330 / w, 250 / h, 1.0)
            if s < 1.0:
                img = img.resize((max(1, int(w * s)), max(1, int(h * s))), Image.LANCZOS)
            if img.height > 250:
                img = img.crop((0, img.height - 250, img.width, img.height))
            self.photo = ImageTk.PhotoImage(img)
            self.view.delete("all")
            self.view.create_image(0, 0, anchor="nw", image=self.photo)
        except Exception as e:
            log(f"show error: {e}")

    def open_folder(self):
        try:
            os.startfile(self.folder)
        except Exception as e:
            log(f"open folder: {e}")

    def finish(self):
        self.stop_flag.set()

    def cancel(self):
        self.cancel_flag.set()
        self.stop_flag.set()
        self.root.after(200, self.root.destroy)

    def record_loop(self):
        canvas = None
        prev = None
        last_new = time.monotonic()
        scrolled = False
        frames = 0
        try:
            while not self.stop_flag.is_set():
                t0 = time.monotonic()
                f = grab(self.zone)
                frames += 1
                if canvas is None:
                    canvas = f
                    last_new = time.monotonic()
                    self.canvas_img = canvas
                    self.status.set("Recording — scroll the zone")
                elif prev is not None:
                    dy, score = find_dy(prev, f)
                    if dy:
                        canvas = np.vstack([canvas, f[f.shape[0] - dy:]])
                        last_new = time.monotonic()
                        scrolled = True
                        self.status.set(f"captured +{dy}px  (total {canvas.shape[0]}px)")
                        self.info.set(f"frames: {frames}  dy: {dy}  score: {score:.1f}")
                        self.canvas_img = canvas
                        log(f"dy={dy} score={score:.1f} canvas={canvas.shape[0]}")
                prev = f
                if scrolled and time.monotonic() - last_new > IDLE_SEC:
                    log("idle stop")
                    break
                dt = 1.0 / FPS - (time.monotonic() - t0)
                if dt > 0:
                    time.sleep(dt)
        except Exception as e:
            log(f"record error: {e}")
            self.status.set(f"error: {e}")
        if self.cancel_flag.is_set():
            log("cancelled")
            return
        if canvas is None:
            return
        try:
            Image.fromarray(canvas).save(self.out_path)
            log(f"saved {self.out_path} {canvas.shape}")
            self.canvas_img = canvas
            self.status.set(f"Saved {os.path.basename(self.out_path)}  {canvas.shape[1]}x{canvas.shape[0]}")
            self.info.set(f"frames: {frames}  → {self.out_path}")
        except Exception as e:
            log(f"save error: {e}")
            self.status.set(f"save error: {e}")


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
        assert diff < 0.5, f"stitch mismatch at step {step}"
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
