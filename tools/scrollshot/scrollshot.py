"""ScrollShot: select a zone, scroll once per frame, stitch into one long PNG.

UI: zone selector -> control panel (status, frame counter, live preview, Stop/Cancel).
"""
import argparse, datetime, os, queue, sys, threading, time
import numpy as np
from PIL import Image, ImageTk

IDLE_LIMIT = 3
SETTLE_MS = 300
MIN_OVERLAP = 0.25
MSE_LIMIT = 12.0


# ---------- imaging ----------

def grab(zone):
    import mss
    x, y, w, h = zone
    with mss.mss() as sct:
        shot = sct.grab({"left": x, "top": y, "width": w, "height": h})
    return np.array(Image.frombytes("RGB", shot.size, shot.bgra, "raw", "BGRX"))


def sig2(img, cols=32):
    g = img.mean(axis=2)
    h, w = g.shape
    cols = max(4, min(cols, w))
    step = max(1, w // cols)
    usable = cols * step
    return g[:, :usable].reshape(h, cols, step).mean(axis=2)


def full_score(prev_img, new_img, s, rows_step=4):
    h = prev_img.shape[0]
    a = prev_img[s:h:rows_step].astype(np.int16)
    b = new_img[:h - s:rows_step].astype(np.int16)
    n = min(len(a), len(b))
    if n == 0:
        return None
    return float(np.abs(a[:n] - b[:n]).mean())


def find_shift(prev_sig, new_sig, verify=None):
    h = len(prev_sig)
    lo = int(h * MIN_OVERLAP)
    best_s, best_score = 0, float("inf")
    for s in range(1, h - lo + 1):
        a = prev_sig[s:]
        b = new_sig[:len(a)]
        score = float(np.abs(a - b).mean())
        if score < best_score:
            best_score, best_s = score, s
    if verify is not None and best_s > 0:
        full = verify(best_s)
        if full is not None:
            return best_s, full
    return best_s, best_score


def best_shift(prev_img, new_img):
    s, score = find_shift(sig2(prev_img), sig2(new_img),
                          verify=lambda s: full_score(prev_img, new_img, s))
    return s, score


def contains(canvas_img, new_img):
    ch, nh = canvas_img.shape[0], new_img.shape[0]
    if nh >= ch:
        return False
    lo = max(0, ch - nh - 200)
    ns = sig2(new_img)
    cs = sig2(canvas_img[lo:])
    best_score = float("inf")
    for off in range(0, len(cs) - len(ns) + 1):
        d = float(np.abs(cs[off:off + len(ns)] - ns).mean())
        if d < best_score:
            best_score = d
    return best_score < MSE_LIMIT


def stitch(frames):
    base = frames[0]
    parts = [base]
    total = base.shape[0]
    canvas_so_far = base
    for i in range(1, len(frames)):
        s, _ = best_shift(frames[i - 1], frames[i])
        if s <= 0:
            continue
        part = frames[i][base.shape[0] - s:]
        if part.shape[0] == 0 or contains(canvas_so_far, frames[i]):
            continue
        parts.append(part)
        total += s
        canvas_so_far = frames[i]
    canvas = np.zeros((total, base.shape[1], 3), dtype=np.uint8)
    y = 0
    for p in parts:
        canvas[y:y + p.shape[0]] = p
        y += p.shape[0]
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
        self.root.geometry("360x420+40+40")
        self.status = tk.StringVar(value="Waiting for scroll…")
        tk.Label(self.root, textvariable=self.status, bg="#1e1e1e", fg="#d0d0d0",
                 font=("Consolas", 10), justify="left", wraplength=340).pack(padx=10, pady=(10, 4), anchor="w")
        self.info = tk.StringVar(value="frames: 1")
        tk.Label(self.root, textvariable=self.info, bg="#1e1e1e", fg="#8fbf8f",
                 font=("Consolas", 9)).pack(padx=10, anchor="w")
        self.canvas = tk.Canvas(self.root, bg="#111111", width=330, height=250, highlightthickness=0)
        self.canvas.pack(padx=10, pady=8)
        self.folder = os.path.dirname(out_path)
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
            q.put(1)

        listener = mouse.Listener(on_scroll=on_scroll)
        listener.start()
        try:
            self.frames.append(grab(self.zone))
            self.events.put(("preview", self.frames[-1]))
            self.events.put(("info", f"frames: {len(self.frames)}  zone {self.zone[2]}x{self.zone[3]}"))
            idle = 0
            while not self.stop_flag.is_set():
                try:
                    q.get(timeout=0.1)
                except queue.Empty:
                    continue
                time.sleep(SETTLE_MS / 1000.0)
                while not q.empty():
                    q.get()
                new = grab(self.zone)
                s, score = best_shift(self.frames[-1], new)
                if score < MSE_LIMIT and s > 0:
                    self.frames.append(new)
                    idle = 0
                    self.events.put(("status", f"captured frame {len(self.frames)} (+{s}px)"))
                    self.events.put(("info", f"frames: {len(self.frames)}  last shift: {s}px"))
                    self.events.put(("preview", new))
                else:
                    idle += 1
                    self.events.put(("status", f"no change ({idle}/{IDLE_LIMIT}) — scroll more or press Stop"))
                    if idle >= IDLE_LIMIT:
                        break
        finally:
            listener.stop()
            if len(self.frames) >= 1:
                self.events.put(("status", "Stitching…"))
                canvas = stitch(self.frames)
                Image.fromarray(canvas).save(self.out_path)
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
    shifts = [(s, round(float(sc), 2)) for s, sc in
              (best_shift(frames[i - 1], frames[i]) for i in range(1, len(frames)))]
    print("expected shift=120 for each step:", shifts)
    assert all(abs(s - 120) <= 1 for s, _ in shifts), "shift detection failed"
    canvas = stitch(frames)
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
