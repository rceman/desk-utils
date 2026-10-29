"""ScrollShot: select a zone, scroll once per frame, stitch into one long PNG."""
import argparse, datetime, os, queue, threading, time
import numpy as np
from PIL import Image, ImageTk

IDLE_LIMIT = 3
SETTLE_MS = 300
SAME_FRAME_DIFF = 2.0
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


def row_hashes(img):
    import hashlib
    return [hashlib.blake2b(np.ascontiguousarray(r).tobytes(), digest_size=8).digest() for r in img]


def rows_equal(a, b, tol=1.0):
    return float(np.abs(a.astype(np.int16) - b.astype(np.int16)).mean()) <= tol


def _verify(canvas, frame, k, forward, tol):
    ch, nh = canvas.shape[0], frame.shape[0]
    n = 0
    while n < k:
        if forward:
            if not rows_equal(canvas[ch - k + n], frame[n], tol):
                return False
        else:
            if not rows_equal(canvas[n], frame[nh - k + n], tol):
                return False
        n += 1
    return True


def find_overlap(canvas, frame, min_rows=40, tol=1.0):
    """Edge-aligned overlap only.

    forward: canvas[ch-k:] == frame[:k]  -> shift = ch-k (content scrolled down)
    reverse: canvas[:k] == frame[nh-k:]  -> shift = -(nh-k)
    Returns (shift, k, forward_bool); (0, 0, True) if nothing found.
    """
    ch, nh = canvas.shape[0], frame.shape[0]
    limit = min(ch, nh, 4000)
    if limit < min_rows:
        return 0, 0, True
    ch_h, nh_h = row_hashes(canvas), row_hashes(frame)
    best = (0, 0, True)
    # forward: match frame[0] against canvas rows
    for i in range(ch - min_rows, -1, -1):
        k = ch - i
        if k > nh or k <= best[1]:
            continue
        if ch_h[i] != nh_h[0]:
            continue
        m = 1
        while m < k and ch_h[i + m] == nh_h[m]:
            m += 1
        if m >= k and k > best[1]:
            best = (ch - k, k, True)
    # reverse: match canvas[0] against frame rows
    for j in range(nh - min_rows, -1, -1):
        k = nh - j
        if k > ch or k <= best[1]:
            continue
        if nh_h[j] != ch_h[0]:
            continue
        m = 1
        while m < k and nh_h[j + m] == ch_h[m]:
            m += 1
        if m >= k and k > best[1]:
            best = (-(nh - k), k, False)
    if best[1] == 0:
        # tolerance fallback: largest edge-aligned overlap
        for k in range(limit, min_rows - 1, -1):
            if _verify(canvas, frame, k, True, tol):
                return ch - k, k, True
            if _verify(canvas, frame, k, False, tol):
                return -(nh - k), k, False
    return best


def full_diff(a, b):
    n = min(a.shape[0], b.shape[0])
    return float(np.abs(a[:n].astype(np.int16) - b[:n].astype(np.int16)).mean())


def stitch(timed_frames, min_rows=40):
    frames = [f for _, f in sorted(timed_frames, key=lambda t: t[0])]
    canvas = frames[0]
    for i in range(1, len(frames)):
        f = frames[i]
        shift, k, forward = find_overlap(canvas, f, min_rows)
        if k >= min_rows:
            new_rows = f[k:] if forward else f[:f.shape[0] - k]
            if new_rows.shape[0] == 0:
                log(f"frame {i}: fully covered (k={k}) -> drop")
                continue
            log(f"frame {i}: overlap k={k} shift={shift} -> append {new_rows.shape[0]} rows")
            canvas = np.vstack([canvas, new_rows]) if forward else np.vstack([new_rows, canvas])
        else:
            log(f"frame {i}: no overlap -> append full frame {f.shape[0]} rows")
            canvas = np.vstack([canvas, f])
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


def intersects(px, py, pw, ph, zone):
    x, y, w, h = zone
    return not (px + pw <= x or px >= x + w or py + ph <= y or py >= y + h)


class Panel:
    def __init__(self, zone, out_path):
        import tkinter as tk
        self.tk = tk
        self.zone = zone
        self.out_path = out_path
        self.frames = []
        self.stop_flag = threading.Event()
        self.cancel_flag = threading.Event()
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
        log(f"panel placed {self.root.geometry()} zone={zone}")

    @staticmethod
    def _place(zone):
        import tkinter as tk
        x, y, w, h = zone
        root = tk.Tk()
        sw, sh = root.winfo_screenwidth(), root.winfo_screenheight()
        root.destroy()
        pw, ph = 380, 470
        cands = [(x + w + 10, y), (x - pw - 10, y), (x, y + h + 10), (x, y - ph - 10),
                 (sw - pw - 10, 10), (10, 10), (sw - pw - 10, sh - ph - 10), (10, sh - ph - 10)]
        best = None
        for cx, cy in cands:
            cx = max(0, min(cx, sw - pw))
            cy = max(0, min(cy, sh - ph))
            if not intersects(cx, cy, pw, ph, zone):
                best = (cx, cy)
                break
            if best is None:
                best = (cx, cy)
        return f"{pw}x{ph}+{best[0]}+{best[1]}"

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
        except Exception as e:
            log(f"poll error: {e}")
        if not self.cancel_flag.is_set() or True:
            self.root.after(60, self.poll)

    def show_preview(self, arr):
        try:
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
        except Exception as e:
            log(f"preview error: {e}")

    def open_folder(self):
        try:
            os.startfile(self.folder)
        except Exception as e:
            log(f"open folder error: {e}")

    def finish(self):
        self.stop_flag.set()

    def cancel(self):
        self.cancel_flag.set()
        self.stop_flag.set()
        self.root.after(200, self.root.destroy)

    def capture_loop(self):
        try:
            from pynput import mouse
        except Exception as e:
            log(f"pynput import failed: {e}")
            self.events.put(("status", f"pynput import failed: {e}"))
            return
        q = queue.Queue()

        def on_scroll(x, y, dx, dy):
            q.put((time.monotonic(), x, y, dx, dy))

        try:
            listener = mouse.Listener(on_scroll=on_scroll)
            listener.start()
            log("mouse listener started")
        except Exception as e:
            log(f"listener failed: {e}")
            self.events.put(("status", f"listener failed: {e}"))
            return
        saved = False
        try:
            first = grab(self.zone)
            self.frames.append((time.monotonic(), first))
            log(f"first frame {first.shape} zone={self.zone}")
            self.events.put(("preview", first))
            self.events.put(("info", f"frames: 1  zone {self.zone[2]}x{self.zone[3]}"))
            self.events.put(("status", "Ready — scroll inside the zone"))
            idle = 0
            while not self.stop_flag.is_set():
                try:
                    ev = q.get(timeout=0.1)
                except queue.Empty:
                    continue
                log(f"wheel event at {ev[1]},{ev[2]} d={ev[3]},{ev[4]}")
                # settle: wait until no new wheel events for SETTLE_MS
                last = time.monotonic()
                while time.monotonic() - last < SETTLE_MS / 1000.0:
                    try:
                        q.get(timeout=0.05)
                        last = time.monotonic()
                    except queue.Empty:
                        pass
                new = grab(self.zone)
                diff = full_diff(self.frames[-1][1], new)
                log(f"capture: diff={diff:.2f}")
                if diff < SAME_FRAME_DIFF:
                    idle += 1
                    self.events.put(("status", f"same picture ({idle}/{IDLE_LIMIT}) — scroll more or press Stop"))
                    if idle >= IDLE_LIMIT:
                        break
                    continue
                idle = 0
                shift, k, forward = find_overlap(self.frames[-1][1], new)
                log(f"overlap k={k} shift={shift}")
                self.frames.append((time.monotonic(), new))
                if k >= 40:
                    self.events.put(("status", f"captured frame {len(self.frames)} (overlap {k}px, shift {shift})"))
                    self.events.put(("info", f"frames: {len(self.frames)}  overlap: {k}px"))
                else:
                    self.events.put(("status", f"captured frame {len(self.frames)} (no overlap, full frame)"))
                    self.events.put(("info", f"frames: {len(self.frames)}  overlap not found"))
                self.events.put(("preview", new))
        except Exception as e:
            log(f"capture loop error: {e}")
            self.events.put(("status", f"error: {e}"))
        finally:
            try:
                listener.stop()
            except Exception:
                pass
            if self.cancel_flag.is_set():
                log("cancelled — nothing saved")
                return
            if not self.frames:
                return
            try:
                self.events.put(("status", "Stitching…"))
                min_rows = max(10, min(40, self.zone[3] // 4))
                canvas = stitch(self.frames, min_rows)
                log(f"stitch: frames={len(self.frames)} result={canvas.shape}")
                Image.fromarray(canvas).save(self.out_path)
                log(f"saved {self.out_path}")
                saved = True
                self.events.put(("preview", canvas))
                self.events.put(("status", f"Saved {os.path.basename(self.out_path)}  {canvas.shape[1]}x{canvas.shape[0]}"))
                self.events.put(("info", f"frames: {len(self.frames)}  → {self.out_path}"))
                self.events.put(("done", f"Saved: {self.out_path}"))
            except Exception as e:
                log(f"stitch/save error: {e}")
                self.events.put(("status", f"save error: {e}"))
        if saved:
            try:
                os.startfile(self.folder)
            except Exception:
                pass


def selftest():
    rng = np.random.default_rng(0)
    tall = (rng.random((1200, 200, 3)) * 255).astype(np.uint8)
    tall[::40] = 30
    frames = [tall[i:i + 400].copy() for i in range(0, 800, 120)]
    ov = [find_overlap(frames[i - 1], frames[i]) for i in range(1, len(frames))]
    print("expected shift=120:", ov)
    assert all(abs(sh - 120) <= 1 and k >= 250 for sh, k, _ in ov), "shift detection failed"
    canvas = stitch([(i, f) for i, f in enumerate(frames)])
    ref = tall[:canvas.shape[0]]
    diff = float(np.abs(canvas.astype(np.int16) - ref.astype(np.int16)).mean())
    print(f"stitched {canvas.shape} vs original {ref.shape} diff={diff:.3f}")
    assert diff < 1.0, "stitching produced mismatched content"
    # repeated header must not be mistaken for the scroll overlap
    a = np.zeros((300, 50, 3), np.uint8); a[:50] = 200
    b = np.zeros((300, 50, 3), np.uint8); b[:50] = 200; b[50:] = 90
    sh, k, fw = find_overlap(a, b)
    print("repeated-header case:", sh, k, fw)
    assert not (k >= 40 and abs(sh) < 40), "repeated header accepted as scroll"
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
