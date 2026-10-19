#!/usr/bin/env python3
"""End-to-end: runs dune nested (a window opens on the current desktop for ~20s)
with dune-term tiles, types into them through the DUNE_DEBUG key injection, and
checks the side effects. Run: cargo build --workspace && python3 -I tests/e2e_nested.py"""

import os
import re
import socket
import subprocess
import sys
import tempfile
import time

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
DUNE = os.path.join(ROOT, "target", "debug", "dune")

# xkb keycodes (evdev + 8) per layout, for the characters the test types.
US = {**dict(zip("qwertyuiop", range(24, 34))), **dict(zip("asdfghjkl", range(38, 47))),
      **dict(zip("zxcvbnm", range(52, 59))), **dict(zip("1234567890", range(10, 20))),
      " ": 65, "\n": 36, "-": 20, "/": 61, ".": 60, ";": 47, "_": 20}
DVORAK = {**US, "'": 24, ",": 25, ".": 26, "p": 27, "y": 28, "f": 29, "g": 30, "c": 31, "r": 32, "l": 33,
          "/": 34, "a": 38, "o": 39, "e": 40, "u": 41, "i": 42, "d": 43, "h": 44, "t": 45, "n": 46, "s": 47,
          "-": 48, "_": 48, ";": 52, "q": 53, "j": 54, "k": 55, "x": 56, "b": 57, "m": 58, "w": 59, "v": 60, "z": 61}
SHIFTED = {"@": "2", ":": ";", "_": "-", ">": "."}
SHIFT, ALT, CTRL, SUPER = 50, 64, 37, 133
ARROW_RIGHT = 114


class Dune:
    def __init__(self, tmp, layout):
        cfg = os.path.join(tmp, "config")
        os.makedirs(os.path.join(cfg, "dune"))
        with open(os.path.join(cfg, "dune", "config"), "w") as f:
            f.write(f"keyboard_layout = us\nkeyboard_variant = {'dvorak' if layout is DVORAK else ''}\n")
        self.layout = layout
        # A desktop application for `:` to find; its Exec creates a file.
        apps = os.path.join(tmp, "data", "applications")
        os.makedirs(apps)
        with open(os.path.join(apps, "dunetest.desktop"), "w") as f:
            # `:dunetes FILE` resolves to it and creates FILE.
            f.write("[Desktop Entry]\nType=Application\nName=Dune Test\nExec=touch %F\n")
        # DUNE_TEST_INPUT=injected: the real keyboard (the nested window may take focus
        # on the desktop while someone types) must not reach the test shells.
        env = dict(os.environ, DUNE_DEBUG="1", DUNE_TEST_INPUT="injected", XDG_CONFIG_HOME=cfg, DUNE_TERMINAL="dune-term",
                   # Via XDG_DATA_DIRS: changing XDG_DATA_HOME would give the user's shell
                   # tools (atuin...) a fresh data directory and a slow first start.
                   XDG_DATA_DIRS=os.path.join(tmp, "data") + ":" + os.environ.get("XDG_DATA_DIRS", "/usr/share"),
                   SHELL="/usr/bin/zsh", RUST_LOG="dune=info,smithay::input=info")
        for var in ("XKB_DEFAULT_LAYOUT", "XKB_DEFAULT_VARIANT"):
            env.pop(var, None)
        self.log = open(os.path.join(tmp, "dune.log"), "w+")
        self.proc = subprocess.Popen([DUNE], env=env, cwd=os.environ["HOME"], stdout=self.log, stderr=subprocess.STDOUT)
        # dune logs its control socket once it listens.
        for _ in range(100):
            self.log.seek(0)
            match = re.search(r'control socket "([^"]+)"', re.sub(r"\x1b\[[0-9;]*m", "", self.log.read()))
            if match:
                self.socket = match.group(1)
                break
            time.sleep(0.1)
        else:
            self.log.seek(0)
            raise RuntimeError("dune did not start:\n" + self.log.read()[-1500:])
        time.sleep(3)  # first terminal and shell

    def keys(self, pairs):
        s = socket.socket(socket.AF_UNIX)
        s.connect(self.socket)
        s.sendall(("keys\t" + " ".join(f"{c}:{'p' if p else 'r'}" for c, p in pairs)).encode())
        s.close()

    def type(self, text):
        for ch in text:
            if ch in SHIFTED:
                k = self.layout[SHIFTED[ch]]
                self.keys([(SHIFT, True), (k, True), (k, False), (SHIFT, False)])
            else:
                k = self.layout[ch]
                self.keys([(k, True), (k, False)])
            time.sleep(0.05)

    def alt(self, ch):
        self.combo([ALT], ch)

    def combo(self, mods, ch):
        k = self.layout[ch]
        self.keys([(m, True) for m in mods] + [(k, True), (k, False)] + [(m, False) for m in reversed(mods)])

    def stop(self):
        self.proc.terminate()
        self.proc.wait(5)
        self.log.seek(0)
        return self.log.read()


def descendants_named(root, name):
    """Processes called `name` below pid `root`."""
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            if open(f"/proc/{entry}/comm").read().strip() != name:
                continue
            pid = int(entry)
            while pid > 1:
                stat = open(f"/proc/{pid}/stat").read()
                pid = int(stat[stat.rindex(")") + 2:].split()[1])
                if pid == root:
                    found.append(int(entry))
                    break
        except OSError:
            continue
    return found


def wait_for(path, seconds=3.0):
    end = time.time() + seconds
    while time.time() < end:
        if os.path.exists(path):
            return True
        time.sleep(0.1)
    return False


def main():
    if not os.path.exists(DUNE) or not os.path.exists(os.path.join(os.path.dirname(DUNE), "dune-term")):
        sys.exit("build first: cargo build --workspace")
    fails = []
    for name, layout in (("us", US), ("dvorak", DVORAK)):
        tmp = tempfile.mkdtemp(prefix="dune-e2e-")
        dune = Dune(tmp, layout)
        try:
            dune.type(f":dunetes {tmp}/one\n")
            if not wait_for(f"{tmp}/one"):
                fails.append(f"[{name}] : in the first tile")
            dune.type(":v\n")
            time.sleep(3)
            dune.type(f":dunetes {tmp}/two\n")
            if not wait_for(f"{tmp}/two"):
                fails.append(f"[{name}] : in a second tile opened with :v")
            # dune-term does the : layer itself: no dune-shell wrapper in its tiles.
            if not descendants_named(dune.proc.pid, "dune-term"):
                fails.append(f"[{name}] no dune-term tiles")
            if descendants_named(dune.proc.pid, "dune-shell"):
                fails.append(f"[{name}] dune-shell wrapper still running in dune-term tiles")
            dune.alt("h")  # focus back to the first (now resized) tile
            time.sleep(0.5)
            dune.type(f":dunetes {tmp}/three\n")
            if not wait_for(f"{tmp}/three"):
                fails.append(f"[{name}] typing after Alt+h into the resized first tile")

            # A GUI program in a new tile takes that tile's place (DUNE_RUN + swallow).
            dune.type(":dune-term:v\n")
            time.sleep(5)
            dune.log.seek(0)
            if "placing window: swallow" not in re.sub(r"\x1b\[[0-9;]*m", "", dune.log.read()):
                fails.append(f"[{name}] :dune-term:v did not swallow its new tile")
            dune.combo([ALT], "q")  # close it; the tile's terminal comes back
            time.sleep(1)

            # Any command takes the split suffix without the picker: it runs in a new tile.
            dune.type(f"touch:v {tmp}/from-split\n")
            if not wait_for(f"{tmp}/from-split", 6):
                fails.append(f"[{name}] `touch:v file` without the picker did not run in a new tile")
            time.sleep(1)

            # The direction indicator (Super+Right) sends the next command to a new tile...
            def terminals():
                dune.log.seek(0)
                return re.sub(r"\x1b\[[0-9;]*m", "", dune.log.read()).count("placing window: terminal")
            before = terminals()
            dune.keys([(SUPER, True), (ARROW_RIGHT, True), (ARROW_RIGHT, False), (SUPER, False)])
            time.sleep(0.3)
            dune.type(f"touch {tmp}/from-direction\n")
            if not wait_for(f"{tmp}/from-direction", 6) or terminals() != before + 1:
                fails.append(f"[{name}] Super+Right then Enter did not open the command in a new tile")
            time.sleep(1)
            # ...once: the next one runs in place.
            before = terminals()
            dune.type(f"touch {tmp}/in-place\n")
            if not wait_for(f"{tmp}/in-place") or terminals() != before:
                fails.append(f"[{name}] the direction indicator was not cleared after use")

            # : launches desktop applications by name.
            dune.type(f":dune {tmp}/from-desktop-entry\n")  # partial name, then the args
            if not wait_for(f"{tmp}/from-desktop-entry", 4):
                fails.append(f"[{name}] : did not launch the desktop entry")

            # Clipboard: an application copies with OSC 52, Ctrl+Shift+V pastes it,
            # and another Wayland client can read it.
            with open(f"{tmp}/osc.sh", "w") as f:
                f.write("printf '\\033]52;c;%s\\007' \"$(printf dune-clip | base64)\"\n")
            dune.type(f"sh {tmp}/osc.sh\n")
            time.sleep(0.5)
            dune.type(f"cat > {tmp}/pasted\n")
            time.sleep(0.5)
            dune.combo([CTRL, SHIFT], "v")
            time.sleep(0.3)
            dune.type("\n")
            dune.combo([CTRL], "d")
            time.sleep(0.5)
            pasted = open(f"{tmp}/pasted").read().strip() if os.path.exists(f"{tmp}/pasted") else None
            if pasted != "dune-clip":
                fails.append(f"[{name}] OSC 52 copy + Ctrl+Shift+V paste: got {pasted!r}")
            dune.type(f"wl-paste > {tmp}/wlpaste\n")
            time.sleep(1.5)
            other = open(f"{tmp}/wlpaste").read().strip() if os.path.exists(f"{tmp}/wlpaste") else None
            if other != "dune-clip":
                fails.append(f"[{name}] another client (wl-paste) reading the clipboard: got {other!r}")
        finally:
            log = dune.stop()
        if f'English ({"Dvorak" if layout is DVORAK else "US"})' not in log:
            fails.append(f"[{name}] keymap not loaded as configured")
        if any(f.startswith(f"[{name}]") for f in fails):
            trace = os.path.join(tempfile.gettempdir(), f"dune-e2e-trace-{name}.log")
            with open(trace, "w") as f:
                f.write(log)
            print(f"--- [{name}] full dune log with the dune-term trace: {trace}")
        if "panicked" in log:
            fails.append(f"[{name}] panic in dune:\n{log[-2000:]}")
    for f in fails:
        print("FAIL:", f)
    if fails:
        sys.exit(1)
    print("e2e nested: all checks passed (us, dvorak)")


main()
