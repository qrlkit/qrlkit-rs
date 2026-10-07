#!/usr/bin/env python3
"""Exercise interactive init through a real PTY, using isolated state/startup files."""
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

BINARY = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/qrlkit").resolve()


def run_interactive(root, answers, success=True, command=("init",)):
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(
            HOME=str(root), ZDOTDIR=str(root), XDG_CONFIG_HOME=str(root),
            SHELL="/bin/zsh", TERM="xterm-256color",
        )
        os.execv(str(BINARY), [str(BINARY), "--config", str(root / "state.yaml"), *command])
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
        # On macOS the PTY may close just before waitpid reports child exit.
        # Keep the original deadline while allowing that exit to become visible.
        while status is None and time.monotonic() < deadline:
            done, result = os.waitpid(pid, os.WNOHANG)
            if done:
                status = result
            else:
                time.sleep(0.01)
        assert status is not None, f"Prompt timed out at step {step}: {transcript!r}"
        assert step == len(answers), f"Missing prompts: {transcript!r}"
        assert (os.waitstatus_to_exitcode(status) == 0) == success, transcript
        assert b"required arguments" not in transcript
    finally:
        if status is None:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        os.close(fd)
    return transcript


# The manual-path fallback makes this test independent of installed browsers.
BROWSER = [
    ("Choose the browser", b"\x1b[B" * 40 + b"\r"),
    ("Browser executable path", b"/bin/sh\r"),
]

with tempfile.TemporaryDirectory(prefix="qrl-init-test-") as directory:
    root = Path(directory)
    startup = root / ".zshrc"
    startup.write_text("export KEEP_ME=yes\n")
    run_interactive(root, BROWSER + [
        ("Choose shell:", b"\r"),
        ("Filehook command", b"\r"),
        ("Dirhook command", b"\r"),
    ])
    state = (root / "state.yaml").read_text()
    assert "shell: zsh" in state
    assert "filehook:" not in state and "dirhook:" not in state
    assert "QRL shell integration" in startup.read_text()
    assert (root / ".zshrc.qrl-backup").read_text() == "export KEEP_ME=yes\n"

    run_interactive(root, BROWSER + [
        ("Choose shell:", b"unsupported\r"),
        ("Unsupported shell.", b"fish\r"),
        ("Filehook command", b"cat $file\r"),
        ("Dirhook command", b"cd $dir && pwd\r"),
    ])
    state = (root / "state.yaml").read_text()
    assert "shell: fish" in state
    assert "filehook: cat $file" in state
    assert "dirhook: cd $dir && pwd" in state
    assert "function qrlkit" in (root / "fish/config.fish").read_text()

    # Cancellation must not replace existing settings or startup files.
    before = (root / "state.yaml").read_bytes()
    run_interactive(root, BROWSER + [
        ("Choose shell:", b"\r"),
        ("Filehook command", b"\x03"),
    ], success=False)
    assert (root / "state.yaml").read_bytes() == before

    # Empty hook inputs restore defaults on subsequent runs as well.
    run_interactive(root, BROWSER + [
        ("Choose shell:", b"\r"),
        ("Filehook command", b"\r"),
        ("Dirhook command", b"\r"),
    ])
    state = (root / "state.yaml").read_text()
    assert "filehook:" not in state and "dirhook:" not in state
    assert startup.read_text().count("# >>> QRL shell integration >>>") == 1

print("Interactive init: defaults, custom settings, validation, cancellation, and rerun passed")


with tempfile.TemporaryDirectory(prefix="qrl-collision-test-") as directory:
    root = Path(directory)
    first = root / "first.toml"
    second = root / "second.toml"
    first.write_text("[qk.git.prs]\nrun = 'printf first'\n")
    second.write_text("[qk.git.prs]\nrun = 'printf second'\n")
    env = dict(os.environ, HOME=str(root), ZDOTDIR=str(root), SHELL="/bin/zsh")

    def run(*args):
        return subprocess.run(
            [str(BINARY), "--config", str(root / "state.yaml"), *args],
            env=env, capture_output=True, text=True, check=True,
        ).stdout

    run("add", str(first))
    run_interactive(root, [
        ("choose the namespace to rename", b"\r"),
        ("Rename qrlkit qk git prs: enter a new name", b"team-prs\r"),
    ], command=("add", str(second)))
    run("reload")
    assert run("qk", "git", "team-prs") == "first"
    assert run("qk", "git", "prs") == "second"
    run_interactive(root, [
        ("choose the namespace to rename", b"\r"),
        ("Rename qrlkit qk: enter a new name", b"team-qk\r"),
    ], command=("set-collision-strategy", "rename"))
    run("set-collision-strategy", "merge")
    run("reload")
    assert run("team-qk", "git", "team-prs") == "first"
    assert run("qk", "git", "prs") == "second"

    third = root / "third.toml"
    third.write_text("[qk.git.other]\nrun = 'printf third'\n")
    run_interactive(root, [
        ("choose the namespace to rename", b"\x1b[B\r"),
        ("Rename qrlkit qk: enter a new name", b"third-qk\r"),
    ], command=("add", str(third), "--collision-strategy", "rename"))
    run("reload")
    assert run("third-qk", "git", "other") == "third"
    state = (root / "state.yaml").read_text()
    assert "\ncollision_strategy: merge\n" in state
    assert "- collision_strategy: rename\n" in state


for extension in ("toml", "yaml", "yml", "json"):
    with tempfile.TemporaryDirectory(prefix="qrl-hint-test-") as directory:
        root = Path(directory)
        source = root / f"hints.{extension}"
        target = root / "repo.txt"
        target.write_text("repo")
        if extension == "toml":
            source.write_text(f"[web]\nplain = '{target}'\n# hint: Opens repo\nrepo = '{target}'\n")
        elif extension in ("yaml", "yml"):
            source.write_text(f"web:\n  plain: {target}\n  # hint: Opens repo\n  repo: {target}\n")
        else:
            source.write_text(json.dumps({"web": {"plain": str(target), "repo": {"url": str(target), "hint": "Opens repo"}}}))
        env = dict(os.environ, HOME=str(root), ZDOTDIR=str(root), SHELL="/bin/zsh")
        subprocess.run(
            [str(BINARY), "--config", str(root / "state.yaml"), "add", str(source)],
            env=env, capture_output=True, check=True,
        )
        transcript = run_interactive(root, [
            ("repo        Opens repo", b"\x1b[B\r"),
        ], command=("web",))
        assert str(target.resolve()).encode() in transcript
        source.write_text(source.read_text().replace("Opens repo", "Updated hint"))
        run_interactive(root, [("repo        Updated hint", b"\x1b[B\r")], command=("web",))
        direct = subprocess.run(
            [str(BINARY), "--config", str(root / "state.yaml"), "web", "repo"],
            env=env, capture_output=True, text=True, check=True,
        )
        assert direct.stdout.strip() == str(target.resolve())

print("Resource hints: rendering, selection, automatic reload, and direct lookup passed")
