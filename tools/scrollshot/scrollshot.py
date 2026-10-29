"""ScrollShot: select a zone, scroll once per frame, stitch into one long PNG.

UI: zone selector -> control panel (status, frame counter, live preview, Stop/Cancel).
"""
import argparse, datetime, os, queue, sys, threading, time
import numpy as np
from PIL import Image, ImageTk

LOG_PATH = None

def log(msg):
    try:
        if LOG_PATH:
            with open(LOG_PATH, "a", encoding="utf-8") as f:
                f.write(f"{datetime.datetime.now():%H:%M:%S.%f}"[:-3] + f" {msg}\n")
    except Exception:
        pass

IDLE_LIMIT = 3
SAME_FRAME_DIFF = 2.0
SETTLE_MS = 300
MIN_OVERLAP = 0.08
MSE_LIMIT = 30.0


# ---------- imaging ----------

def grab(zone):
    import mss
    x, y, w, h = zone
    with mss.mss() as sct:
        shot = sct.grab({"left": x, "top": y, "width": w, "height": h})
    return np.array(Image.frombytes("RGB", shot.size, shot.bgra, "raw", "BGRX"))


def row_hashes(img):
    """Exact per-row pixel hashes (fast, 1px-accurate row identity)."""
    import hashlib
    return [hashlib.blake2b(np.ascontiguousarray(r).tobytes(), digest_size=8).digest()
            for r in img]


def rows_equal(a, b, tol=1.0):
    return float(np.abs(a.astype(np.int16) - b.astype(np.int16)).mean()) <= tol


def find_overlap(canvas_img, new_img, min_rows=40, tol=1.0):
    """Largest block where canvas[i:i+k] == new[j:j+k] (pixel-wise).

    Returns (shift, k): shift = i - j (positive when new content scrolled down).
    """
    ch, nh = canvas_img.shape[0], new_img.shape[0]
    if ch == 0 or nh == 0:
        return 0, 0
    ch_h = row_hashes(canvas_img)
    nh_h = row_hashes(new_img)
    idx = {}
    for i, h in enumerate(ch_h):
        idx.setdefault(h, []).append(i)
    best_shift, best_k = 0, 0
    for j, h in enumerate(nh_h[:min(nh, ch)]):
        for i in idx.get(h, ()):
            k = 0
            while i + k < ch and j + k < nh and ch_h[i + k] == nh_h[j + k]:
                k += 1
            # tolerate anti-aliasing: verify/extend with pixel tolerance
            if k >= min_rows and k > best_k:
                best_shift, best_k = i - j, k
    if best_k == 0:
        # fallback: tolerance-based scan on a coarse grid
        for shift in range(-(nh - min_rows), ch - min_rows):
            i0 = max(0, shift)
            j0 = max(0, -shift)
            k = 0
            while (i0 + k < ch and j0 + k < nh and k < 4000
                   and rows_equal(canvas_img[i0 + k], new_img[j0 + k], tol)):
                k += 1
            if k >= min_rows and k > best_k:
                best_shift, best_k = shift, k
    return best_shift, best_k


def stitch(timed_frames, min_rows=40):
    """Stitch frames in capture-time order; drop content already present."""
    frames = [f for _, f in sorted(timed_frames, key=lambda t: t[0])]
    canvas = frames[0]
    for i in range(1, len(frames)):
        shift, k = find_overlap(canvas, frames[i], min_rows)
        if k >= min_rows:
            new_rows = frames[i][k:] if shift >= 0 else frames[i][:frames[i].shape[0] - k]
            if new_rows.shape[0] == 0:
                log(f"frame {i}: fully covered (k={k}) -> drop")
                continue
            log(f"frame {i}: overlap k={k} shift={shift} -> append {new_rows.shape[0]} rows")
            canvas = np.vstack([canvas, new_rows]) if shift >= 0 else np.vstack([new_rows, canvas])
        else:
            # no overlap: keep whole frame (content changed non-linearly)
            log(f"frame {i}: no overlap -> append full frame {frames[i].shape[0]} rows")
            canvas = np.vstack([canvas, frames[i]])
    return canvas


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


# ---------- control panel ----------

class Panel:
    def __init__(self, zone, out_path):
        import tkinter as tk
        self.tk = tk
        self.zone = zone
        self.out_path = out_path
        self.frames = []
        self.stop_flag = threading.Event()
        self.events = queue.Queue()
        self.root = tk.Tk()
        self.root.title("ScrollShot")
        self.root.attributes("-topmost", True)
        self.root.configure(bg="#1e1e1e")
        self.root.geometry(self._place(zone))
        self.status = tk.StringVar(value="Waiting for scroll…")
        tk.Label(self.root, textvariable=self.status, bg="#1e1e1e", fg="#d0d0d0",
                 font=("Consolas", 10), justify="left", wraplength=340).pack(padx=10, pady=(10, 4), anchor="w")
        self.info = tk.StringVar(value="frames: 1")
        tk.Label(self.root, textvariable=self.info, bg="#1e1e1e", fg="#8fbf8f",
                 font=("Consolas", 9)).pack(padx=10, anchor="w")
        self.canvas = tk.Canvas(self.root, bg="#111111", width=330, height=250, highlightthickness=0)
        self.canvas.pack(padx=10, pady=8)
        global LOG_PATH
        self.folder = os.path.dirname(out_path)
        os.makedirs(self.folder, exist_ok=True)
        LOG_PATH = os.path.join(self.folder, "scrollshot.log")
        log(f"panel placed {self.root.geometry()} zone={zone}")
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
        self.worker = threading.Thread(target=self.capture_loop, daemon=True)

    @staticmethod
    def _place(zone):
        import tkinter as tk
        x, y, w, h = zone
        root = tk.Tk()
        sw, sh = root.winfo_screenwidth(), root.winfo_screenheight()
        root.destroy()
        pw, ph = 380, 470
        if sw - (x + w) >= pw + 20:
            px, py = x + w + 10, y
        elif x >= pw + 20:
            px, py = x - pw - 10, y
        elif sh - (y + h) >= ph + 20:
            px, py = x, y + h + 10
        else:
            px, py = sw - pw - 10, 10
        px = max(0, min(px, sw - pw))
        py = max(0, min(py, sh - ph))
        return f"{pw}x{ph}+{px}+{py}"

    def start(self):
        self.worker.start()
        self.root.after(60, self.poll)
        self.root.mainloop()

    def poll(self):
        try:
            while True:
                kind, payload = self.events.get_nowait()
                if kind == "status":
                    self.status.set(payload)
                elif kind == "info":
                    self.info.set(payload)
                elif kind == "preview":
                    self.show_preview(payload)
                elif kind == "done":
                    self.status.set(payload)
                    self.root.after(1200, self.root.destroy)
                    return
        except queue.Empty:
            pass
        if not self.stop_flag.is_set():
            self.root.after(60, self.poll)

    def show_preview(self, arr):
        img = Image.fromarray(arr)
        w, h = img.size
        scale = min(330 / w, 250 / h, 1.0)
        if scale < 1.0:
            img = img.resize((max(1, int(w * scale)), max(1, int(h * scale))), Image.LANCZOS)
        if img.height > 250:
            img = img.crop((0, img.height - 250, img.width, img.height))
        self.photo = ImageTk.PhotoImage(img)
        self.canvas.delete("all")
        self.canvas.create_image(0, 0, anchor="nw", image=self.photo)

    def open_folder(self):
        os.makedirs(self.folder, exist_ok=True)
        os.startfile(self.folder)

    def finish(self):
        self.stop_flag.set()

    def cancel(self):
        self.stop_flag.set()
        self.events.put(("status", "Cancelled"))
        self.root.after(300, self.root.destroy)

    def capture_loop(self):
        from pynput import mouse
        q = queue.Queue()

        def on_scroll(x, y, dx, dy):
            log(f"wheel event at {x},{y} d={dx},{dy}")
            q.put(1)

        try:
            listener = mouse.Listener(on_scroll=on_scroll)
            listener.start()
            log("mouse listener started")
        except Exception as e:
            log(f"listener failed: {e}")
            self.events.put(("status", f"listener failed: {e}"))
            return
        try:
            self.frames.append((time.time(), grab(self.zone)))
            log(f"first frame {self.frames[-1][1].shape}")
            self.events.put(("status", "Ready — scroll inside the zone"))
            self.events.put(("preview", self.frames[-1]))
            self.events.put(("info", f"frames: {len(self.frames)}  zone {self.zone[2]}x{self.zone[3]}"))
            idle = 0
            while not self.stop_flag.is_set():
                try:
                    q.get(timeout=0.1)
                except queue.Empty:
                    continue
                time.sleep(SETTLE_MS / 1000.0)
                new = grab(self.zone)
                diff = full_diff(self.frames[-1][1], new)
                log(f"scroll event: diff={diff:.2f}")
                if diff < SAME_FRAME_DIFF:
                    idle += 1
                    self.events.put(("status", f"same picture ({idle}/{IDLE_LIMIT}) — scroll more or press Stop"))
                    if idle >= IDLE_LIMIT:
                        break
                    continue
                idle = 0
                shift, k = find_overlap(self.frames[-1][1], new)
                log(f"overlap k={k} shift={shift}")
                self.frames.append((time.time(), new))
                if k >= 40:
                    self.events.put(("status", f"captured frame {len(self.frames)} (overlap {k}px, shift {shift})"))
                    self.events.put(("info", f"frames: {len(self.frames)}  overlap: {k}px"))
                else:
                    self.events.put(("status", f"captured frame {len(self.frames)} (no overlap, full frame)"))
                    self.events.put(("info", f"frames: {len(self.frames)}  overlap not found"))
                self.events.put(("preview", new))
        finally:
            listener.stop()
            if len(self.frames) >= 1:
                self.events.put(("status", "Stitching…"))
                canvas = stitch(self.frames)
                log(f"stitch: frames={len(self.frames)} result={canvas.shape}")
                Image.fromarray(canvas).save(self.out_path)
                log(f"saved {self.out_path}")
                self.events.put(("preview", canvas))
                self.events.put(("status", f"Saved {os.path.basename(self.out_path)}  {canvas.shape[1]}x{canvas.shape[0]}"))
                self.events.put(("info", f"frames: {len(self.frames)}  → {self.out_path}"))
                try:
                    os.startfile(self.folder)
                except Exception:
                    pass
                self.events.put(("done", f"Saved: {self.out_path}"))


# ---------- selftest ----------

def selftest():
    rng = np.random.default_rng(0)
    tall = (rng.random((1200, 200, 3)) * 255).astype(np.uint8)
    tall[::40] = 30
    frames = [tall[i:i + 400].copy() for i in range(0, 800, 120)]
    ov = [find_overlap(frames[i - 1], frames[i]) for i in range(1, len(frames))]
    print("expected shift=120 for each step:", ov)
    assert all(abs(sh - 120) <= 1 and k >= 250 for sh, k in ov), "shift detection failed"
    canvas = stitch([(i, f) for i, f in enumerate(frames)])
    ref = tall[:canvas.shape[0]]
    diff = float(np.abs(canvas.astype(np.int16) - ref.astype(np.int16)).mean())
    print(f"stitched {canvas.shape} vs original {ref.shape} diff={diff:.3f}")
    assert diff < 1.0, "stitching produced mismatched content"
    print("selftest OK (no duplicated content)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()
    if args.selftest:
        selftest()
        return
    zone = select_zone()
    if not zone:
        print("cancelled")
        return
    out = args.out or os.path.join(os.path.expanduser("~"), "Pictures", "Screenshots", "ScrollShot",
                                   f"scrollshot_{datetime.datetime.now():%Y%m%d_%H%M%S}.png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Panel(zone, out).start()


if __name__ == "__main__":
    main()
