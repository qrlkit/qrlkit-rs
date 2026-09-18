#!/usr/bin/env python3
"""Exercise interactive init through a real PTY, using isolated state/startup files."""
import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import sys
import tempfile
import termios
import time

BINARY = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/qrlkit").resolve()


def run_init(root, answers, success=True):
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(
            HOME=str(root), ZDOTDIR=str(root), XDG_CONFIG_HOME=str(root),
            SHELL="/bin/zsh", TERM="xterm-256color",
        )
        os.execv(str(BINARY), [str(BINARY), "--config", str(root / "state.yaml"), "init"])
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 180, 0, 0))
    transcript = b""
    pending = b""
    query_tail = b""
    step = 0
    status = None
    deadline = time.monotonic() + 20
    try:
        while time.monotonic() < deadline:
            if select.select([fd], [], [], 0.1)[0]:
                try:
                    chunk = os.read(fd, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    break
                if not chunk:
                    break
                transcript += chunk
                pending += chunk
                queries = query_tail + chunk
                for _ in range(queries.count(b"\x1b[6n")):
                    os.write(fd, b"\x1b[1;1R")
                query_tail = queries[-3:]
                visible = re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", pending)
                visible = b"".join(visible.split())
                if step < len(answers) and b"".join(answers[step][0].encode().split()) in visible:
                    os.write(fd, answers[step][1])
                    pending = b""
                    step += 1
            done, result = os.waitpid(pid, os.WNOHANG)
            if done:
                status = result
                break
        if status is None:
            done, result = os.waitpid(pid, os.WNOHANG)
            if done:
                status = result
        assert status is not None, f"Prompt timed out at step {step}: {transcript!r}"
        assert step == len(answers), f"Missing prompts: {transcript!r}"
        assert (os.waitstatus_to_exitcode(status) == 0) == success, transcript
        assert b"required arguments" not in transcript
    finally:
        if status is None:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        os.close(fd)


# The manual-path fallback makes this test independent of installed browsers.
BROWSER = [
    ("Choose the browser", b"\x1b[B" * 40 + b"\r"),
    ("Browser executable path", b"/bin/sh\r"),
]

with tempfile.TemporaryDirectory(prefix="qrl-init-test-") as directory:
    root = Path(directory)
    startup = root / ".zshrc"
    startup.write_text("export KEEP_ME=yes\n")
    run_init(root, BROWSER + [
        ("Choose shell:", b"\r"),
        ("Filehook command", b"\r"),
        ("Dirhook command", b"\r"),
    ])
    state = (root / "state.yaml").read_text()
    assert "shell: zsh" in state
    assert "filehook:" not in state and "dirhook:" not in state
    assert "QRL shell integration" in startup.read_text()
    assert (root / ".zshrc.qrl-backup").read_text() == "export KEEP_ME=yes\n"

    run_init(root, BROWSER + [
        ("Choose shell:", b"unsupported\r"),
        ("Unsupported shell.", b"fish\r"),
        ("Filehook command", b"cat file\r"),
        ("Dirhook command", b"cd dir && pwd\r"),
    ])
    state = (root / "state.yaml").read_text()
    assert "shell: fish" in state
    assert "filehook: cat file" in state
    assert "dirhook: cd dir && pwd" in state
    assert "function qrlkit" in (root / "fish/config.fish").read_text()

    # Cancellation must not replace existing settings or startup files.
    before = (root / "state.yaml").read_bytes()
    run_init(root, BROWSER + [
        ("Choose shell:", b"\r"),
        ("Filehook command", b"\x03"),
    ], success=False)
    assert (root / "state.yaml").read_bytes() == before

    # Empty hook inputs restore defaults on subsequent runs as well.
    run_init(root, BROWSER + [
        ("Choose shell:", b"\r"),
        ("Filehook command", b"\r"),
        ("Dirhook command", b"\r"),
    ])
    state = (root / "state.yaml").read_text()
    assert "filehook:" not in state and "dirhook:" not in state
    assert startup.read_text().count("# >>> QRL shell integration >>>") == 1

print("Interactive init: defaults, custom settings, validation, cancellation, and rerun passed")
