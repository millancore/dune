#!/usr/bin/env python3
"""Drives `dune shell` (the : layer) in a real pty around bash and zsh, with a
fake compositor socket that records the requests. Needs a built binary:
    cargo build && python3 -I tests/shell_wrapper.py"""

import os
import pty
import select
import shutil
import socket
import sys
import tempfile
import threading
import time

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
DUNE = os.environ.get("DUNE_BIN", os.path.join(ROOT, "target", "debug", "dune"))
TILE = "7"


class FakeCompositor:
    def __init__(self, path):
        self.requests = []
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.bind(path)
        self.sock.listen()
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                return
            data = b""
            while chunk := conn.recv(4096):
                data += chunk
            conn.close()
            self.requests.append(data.decode())


def drain(fd, seconds):
    out, end = b"", time.time() + seconds
    while time.time() < end:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if ready:
            try:
                out += os.read(fd, 4096)
            except OSError:
                break
    return out


class Session:
    def __init__(self, tmp, shell, shell_args, extra_env=None):
        env = {
            "PATH": f"{tmp}/bin:/usr/bin:/bin",
            "HOME": tmp,
            "TERM": "xterm-256color",
            "DUNE_SOCKET": f"{tmp}/dune.sock",
            "DUNE_TILE": TILE,
            "DUNE_USER_SHELL": shell,
            "PS1": "$ ",
            "ZDOTDIR": f"{tmp}/zdot",
            # `:` offers desktop applications: only the test's own.
            "XDG_DATA_DIRS": f"{tmp}/data",
        }
        env.update(extra_env or {})
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(f"{tmp}/work")
            os.execve(DUNE, ["dune", "shell", *shell_args], env)
        self.screen = drain(self.fd, 1.5)

    def type(self, text, per_key=0.15):
        for ch in text:
            os.write(self.fd, ch.encode())
            self.screen += drain(self.fd, per_key)
        self.screen += drain(self.fd, 0.5)

    def text(self):
        return self.screen.decode(errors="replace")

    def close(self):
        os.write(self.fd, b"\x03exit\r")
        drain(self.fd, 0.5)
        try:
            os.close(self.fd)
        except OSError:
            pass
        os.waitpid(self.pid, 0)


def main():
    if not os.path.exists(DUNE):
        sys.exit("build first: cargo build")
    tmp = tempfile.mkdtemp()
    fails = []

    def check(cond, what, session=None):
        if not cond:
            fails.append(what + (f"\n    screen tail: {session.text()[-400:]!r}" if session else ""))

    try:
        os.makedirs(f"{tmp}/bin")
        os.makedirs(f"{tmp}/work")
        open(f"{tmp}/work/index.php", "w").close()
        ran = f"{tmp}/ran"
        with open(f"{tmp}/bin/dunetestprog", "w") as f:
            f.write(f'#!/bin/sh\necho "RAN $*" >> {ran}\n')
        os.chmod(f"{tmp}/bin/dunetestprog", 0o755)
        os.makedirs(f"{tmp}/data/applications")
        with open(f"{tmp}/data/applications/dunetestprog.desktop", "w") as f:
            f.write("[Desktop Entry]\nType=Application\nName=Dune Test Prog\nExec=dunetestprog %F\n")
        fake = FakeCompositor(f"{tmp}/dune.sock")

        # Startup files that run a program, as real configs do: the wrapper must not
        # mistake that for the end of a `:prog` or type DUNE_RUN before the prompt.
        startup = "sleep 0.3; echo rc-loaded\nPS1='$ '\n"
        with open(f"{tmp}/bashrc", "w") as f:
            f.write(startup)
        os.makedirs(f"{tmp}/zdot")
        with open(f"{tmp}/zdot/.zshrc", "w") as f:
            f.write(startup)
        shells = [("/bin/bash", ["--noprofile", "--rcfile", f"{tmp}/bashrc"]), ("/bin/zsh", [])]
        for shell, args in shells:
            name = os.path.basename(shell)
            work = f"{tmp}/work"

            def reset():
                fake.requests.clear()
                if os.path.exists(ran):
                    os.remove(ran)

            # 1. live list + run in place, with swallow / unswallow around it
            reset()
            s = Session(tmp, shell, args)
            s.type(":dunetest")
            check("Dune Test Prog" in s.text(), f"[{name}] live list shows the app while typing", s)
            s.type("\r")
            time.sleep(0.5)
            s.screen += drain(s.fd, 0.5)
            check(os.path.exists(ran) and open(ran).read().strip() == "RAN", f"[{name}] :prog runs in place", s)
            check(any(r.startswith(f"swallow\t{TILE}\t") for r in fake.requests), f"[{name}] swallow sent: {fake.requests}")
            # A program this short may end before the wrapper sees it running; then
            # the swallow ends with the next Enter at the latest.
            s.type("\r")
            check(f"unswallow\t{TILE}" in fake.requests, f"[{name}] unswallow once the program ended: {fake.requests}")

            # 1b. a program that keeps running: unswallow as soon as it ends, no Enter needed
            reset()
            s.type(":sleep 1\r")
            time.sleep(2.0)
            s.screen += drain(s.fd, 0.5)
            check(f"unswallow\t{TILE}" in fake.requests, f"[{name}] unswallow when a running program ends: {fake.requests}")

            # 2. split: nothing runs here, the compositor is asked to open a tile; Tab completes files
            reset()
            s.type(":dunetestprog:v ind\t\r")
            check(f"open\tr\t{TILE}\t{work}\tdunetestprog index.php" in fake.requests,
                  f"[{name}] split request with completed file: {fake.requests}", s)
            check(not os.path.exists(ran), f"[{name}] split does not run the program locally")

            # 3. : in the middle of a line is ordinary text
            reset()
            s.type("echo mid:dle\r")
            check("mid:dle" in s.text().split("echo mid:dle")[-1], f"[{name}] : inside a line goes to the shell", s)
            check(fake.requests == [], f"[{name}] no request for an ordinary line: {fake.requests}")

            # 4. while a program runs, : belongs to it
            reset()
            s.type("cat\r")
            s.type(":inside\r")
            check(":inside" in s.text().split("cat")[-1], f"[{name}] : passes through to a running program", s)
            s.type("\x04")
            check(fake.requests == [], f"[{name}] no request while a program runs: {fake.requests}")

            # 5. Esc cancels, the prompt keeps working
            reset()
            s.type(":dune\x1b")
            s.type("echo after-cancel\r")
            check(s.text().count("after-cancel") >= 2, f"[{name}] shell works after cancelling the picker", s)
            check(fake.requests == [], f"[{name}] cancel sends nothing: {fake.requests}")
            s.close()

            # 6. a tile opened with :prog:v runs its command first (DUNE_RUN)
            reset()
            s = Session(tmp, shell, args, {"DUNE_RUN": "dunetestprog first"})
            time.sleep(1.0)
            s.screen += drain(s.fd, 0.3)
            check(os.path.exists(ran) and "RAN first" in open(ran).read(), f"[{name}] DUNE_RUN runs at start", s)
            check(any(r.startswith(f"swallow\t{TILE}\t") for r in fake.requests), f"[{name}] DUNE_RUN swallow: {fake.requests}")
            check(f"unswallow\t{TILE}" not in fake.requests[:1], f"[{name}] startup programs don't end the swallow early: {fake.requests}")
            check(s.text().find("rc-loaded") < s.text().find("dunetestprog first"),
                  f"[{name}] DUNE_RUN is typed after the shell finished starting", s)
            s.close()

        for f in fails:
            print("FAIL:", f)
        if fails:
            sys.exit(1)
        print(f"shell wrapper: all checks passed ({', '.join(os.path.basename(s) for s, _ in shells)})")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


main()
