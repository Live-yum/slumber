#!/usr/bin/env python3
"""CI-only black-box terminal checks. pywinpty/ptyprocess/pyte are NOT runtime dependencies."""
from __future__ import annotations
import argparse
import contextlib
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import pyte

if os.name == "nt":
    from winpty import PTY
else:
    from ptyprocess import PtyProcessUnicode as PtyProcess

TRANSCRIPTS = []

class Session:
    def __init__(self, argv, cwd, env):
        self.pending = queue.Queue()
        self.buffer = ""
        self.transcript = ""
        self.reader = None
        self.screen = pyte.Screen(120, 32)
        self.stream = pyte.Stream(self.screen)
        if os.name == "nt":
            # Use the public nonblocking PTY API directly. PtyProcess adds a
            # socket-forwarding thread; in 3.0.5 isalive() sets closed=True on
            # child exit, so close() may skip releasing the blocked reader.
            # There is no Python reader thread or forwarding socket to leak.
            self.process = PTY(120, 32)
            self.process.spawn(
                argv[0], cmdline=" " + subprocess.list2cmdline(argv[1:]),
                cwd=str(cwd), env="\0".join(f"{k}={v}" for k, v in env.items()) + "\0",
            )
        else:
            self.process = PtyProcess.spawn(argv, cwd=str(cwd), env=env, dimensions=(32, 120))
            def reader():
                try:
                    while True:
                        text = self.process.read(4096)
                        if text:
                            self.pending.put(text)
                except (EOFError, OSError):
                    self.pending.put(None)
            self.reader = threading.Thread(target=reader, daemon=True)
            self.reader.start()

    def pump(self):
        if os.name == "nt":
            # Bound work per iteration so a noisy process cannot defeat the
            # expect/exit deadlines. UTF-16/UTF-8 decoding belongs to PTY.
            for _ in range(128):
                try:
                    text = self.process.read(blocking=False)
                except Exception:
                    # Native PTY reports EOF as WinptyError, not empty text.
                    # Accept only confirmed EOF; finish() still checks exit 0
                    # and the caller still checks the exact saved file bytes.
                    if self.process.iseof():
                        break
                    raise
                if not text:
                    break
                self.buffer += text
                self.transcript += text
                self.stream.feed(text)
        else:
            while True:
                try:
                    text = self.pending.get_nowait()
                except queue.Empty:
                    break
                if text is not None:
                    self.buffer += text
                    self.transcript += text
                    self.stream.feed(text)

    def send(self, text):
        self.pump()
        self.buffer = ""
        self.process.write(text)

    def expect(self, text):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            self.pump()
            if text in "\n".join(self.screen.display):
                return
            if not self.process.isalive():
                self.pump()
                raise AssertionError(f"Program exited before terminal showed {text!r}")
            time.sleep(0.02)
        raise AssertionError(f"Terminal did not show {text!r}; see builtin-terminal.log")

    def keep_visible(self, text, seconds=0.35):
        # This deliberately holds the menu across watcher ticks. Sleeping
        # BEFORE opening the menu would hide the duplicate-reload regression.
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.pump()
            assert text in "\n".join(self.screen.display), f"Open menu lost: {text!r}"
            time.sleep(0.02)

    def replace_document(self, text):
        self.send("\x01")  # Ctrl+A
        self.send("\x1b[200~" + text + "\x1b[201~")

    def finish(self):
        deadline = time.monotonic() + 20
        while self.process.isalive() and time.monotonic() < deadline:
            self.pump()
            time.sleep(0.02)
        assert not self.process.isalive(), "Program did not exit after closing the editor/TUI"
        status = self.process.get_exitstatus() if os.name == "nt" else self.process.exitstatus
        assert status == 0, status

    def close(self):
        try:
            if os.name == "nt":
                self.pump()
                # Passing cases already observed exit code zero in finish().
                # A failed assertion must not leave the synthetic child alive.
                if self.process.isalive():
                    os.kill(self.process.pid, signal.SIGTERM)
                deadline = time.monotonic() + 5
                while self.process.isalive() and time.monotonic() < deadline:
                    self.pump()
                    time.sleep(0.01)
                assert not self.process.isalive(), "Terminal child did not stop"
                self.pump()
                self.process = None  # Release native ConPTY handles.
            else:
                self.process.close(force=True)
                self.reader.join(timeout=5)
                self.pump()
                assert not self.reader.is_alive(), "Terminal reader did not stop"
        finally:
            TRANSCRIPTS.append(self.transcript + "\n--- FINAL SCREEN ---\n" + "\n".join(self.screen.display))

@contextlib.contextmanager
def session(argv, cwd, env):
    process = Session(argv, cwd, env)
    try:
        yield process
    finally:
        process.close()


def wait_file(path, expected):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if path.read_bytes() == expected.encode("utf-8"):
            return
        time.sleep(0.02)
    actual = path.read_bytes()
    wanted = expected.encode("utf-8")
    raise AssertionError(f"Synthetic fixture byte mismatch: actual={actual!r}, expected={wanted!r}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="slumber-native-terminal-") as temporary:
        root = Path(temporary)
        portable = root / "中文 空格 portable"
        portable.mkdir()
        executable = portable / binary.name
        shutil.copy2(binary, executable)
        elsewhere = root / "other cwd"
        elsewhere.mkdir()
        collection = portable / "slumber.yml"
        config = portable / "config.yml"
        original = "name: BEFORE-EDIT\nrequests:\n  test:\n    method: GET\n    url: http://127.0.0.1:1/original\n"
        updated = "name: AFTER-EDIT\nrequests:\n  test:\n    method: GET\n    url: http://127.0.0.1:1/edited\n# 中文 Unicode 😀\n"
        cli_updated = updated + "# CLI-EDIT-SAVED\n"
        collection.write_bytes(original.encode("utf-8"))
        # Even an old explicit vim setting must not escape portable mode.
        config.write_bytes(b"editor: vim\npager: nonexistent-pager\npersist: false\n")
        env = {key: value for key, value in os.environ.items()
               if key.upper() not in ("PATH", "EDITOR", "VISUAL", "PAGER", "SLUMBER_DATA_DIRECTORY", "SLUMBER_CONFIG_PATH")}
        env.update(PATH="", EDITOR="nonexistent-editor", VISUAL="nonexistent-editor", PAGER="nonexistent-pager", TERM="xterm-256color")
        command = [str(executable), "--portable"]
        with session(command, elsewhere, env) as terminal:
            terminal.expect("127.0.0.1")
            terminal.send("x")
            terminal.expect("Edit Recipe")
            terminal.send("\r")
            terminal.expect("Built-in editor")
            terminal.replace_document(updated)
            terminal.send("\x1bOQ")  # F2: save and close
            wait_file(collection, updated)
            # The completed reload notification replaces the footer for five
            # seconds. Act now, while a delayed watcher could still arrive;
            # waiting for the collection title would hide the original race.
            terminal.expect("Reloaded collection")
            terminal.send("x")
            terminal.expect("Edit Recipe")
            terminal.keep_visible("Edit Recipe")
            terminal.send("\r")
            terminal.expect("Built-in editor")
            terminal.replace_document("DO-NOT-SAVE\n")
            terminal.send("\x1b")
            terminal.expect("Unsaved changes")
            terminal.send("y")
            terminal.expect("Editor closed")
            assert collection.read_bytes() == updated.encode("utf-8")
            terminal.send("\x03")  # normal TUI quit, outside the editor
            terminal.finish()
        with session(command + ["collection", "--edit"], elsewhere, env) as terminal:
            terminal.expect("Built-in editor")
            terminal.replace_document(cli_updated)
            terminal.send("\x1bOQ")
            terminal.finish()
        assert collection.read_bytes() == cli_updated.encode("utf-8")
        new_config = "editor: builtin\npager: builtin\npersist: false\n# 配置已保存\n"
        with session(command + ["config", "--edit"], elsewhere, env) as terminal:
            terminal.expect("Built-in editor")
            terminal.replace_document(new_config)
            terminal.send("\x1bOQ")
            terminal.finish()
        assert config.read_bytes() == new_config.encode("utf-8")
        assert not list(elsewhere.glob("*.sqlite"))
    print("PASS: packaged executable, real terminal x-menu editing, Unicode save/cancel, CLI collection/config editing, empty PATH, no external editor")

if __name__ == "__main__":
    try:
        main()
    finally:
        Path("builtin-terminal.log").write_text("\n\n--- SESSION ---\n\n".join(TRANSCRIPTS), encoding="utf-8")
