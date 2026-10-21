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


def run_interactive(root, answers, success=True, command=("init",), directory_channel=False, colors=False):
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(
            HOME=str(root), ZDOTDIR=str(root), XDG_CONFIG_HOME=str(root),
            SHELL="/bin/zsh", TERM="xterm-256color",
        )
        if colors:
            os.environ.pop("NO_COLOR", None)
        if directory_channel:
            os.environ["QRL_CD_FILE"] = str(root / "directory-channel")
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


for extension, hint_style in (
    ("toml", "above"), ("toml", "inline"),
    ("yaml", "above"), ("yaml", "inline"),
    ("yml", "above"), ("yml", "inline"),
    ("json", "field"),
):
    with tempfile.TemporaryDirectory(prefix="qrl-hint-test-") as directory:
        root = Path(directory)
        source = root / f"hints.{extension}"
        target = root / "repo.txt"
        target.write_text("repo")
        if extension == "toml":
            repo = f"# hint: Opens repo\nrepo = '{target}'\n" if hint_style == "above" else f"repo = '{target}' # hint: Opens repo\n"
            source.write_text(f"[web]\nplain = '{target}'\n{repo}")
        elif extension in ("yaml", "yml"):
            repo = f"  # hint: Opens repo\n  repo: {target}\n" if hint_style == "above" else f"  repo: {target} # hint: Opens repo\n"
            source.write_text(f"web:\n  plain: {target}\n{repo}")
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

with tempfile.TemporaryDirectory(prefix="qrl-enum-test-") as directory:
    root = Path(directory)
    source = root / "tasks.toml"
    source.write_text('''
[constants]
environments = ["dev", "staging", "prod"]
[deploy]
run = 'printf "%s/%s" "$1" "$2" > selected'
args = [
    { name = "environment", enum = { ref = "constants.environments" } },
    { name = "region", enum = ["eu", "us"] },
]
''')
    env = dict(os.environ, HOME=str(root), ZDOTDIR=str(root), SHELL="/bin/zsh")
    subprocess.run(
        [str(BINARY), "--config", str(root / "state.yaml"), "add", str(source)],
        env=env, capture_output=True, check=True,
    )
    run_interactive(root, [
        ("environment:", b"\x1b[B\r"),
        ("region:", b"\x1b[B\r"),
    ], command=("deploy",))
    assert (root / "selected").read_text() == "staging/us"
    run_interactive(root, [("region:", b"\r")], command=("deploy", "prod"))
    assert (root / "selected").read_text() == "prod/eu"
    (root / "selected").unlink()
    run_interactive(root, [("environment:", b"\x1b")], success=False, command=("deploy",))
    assert not (root / "selected").exists()

print("Enum arguments: full and partial selection, ordering, and cancellation passed")

with tempfile.TemporaryDirectory(prefix="qrl-text-arg-test-") as directory:
    root = Path(directory)
    source = root / "tasks.toml"
    source.write_text('''
[prepare]
run = 'printf "%s/%s" "$1" "$2" > selected'
args = [
    { name = "type", enum = ["major", "minor", "patch"] },
    { name = "note" },
]
''')
    env = dict(os.environ, HOME=str(root), ZDOTDIR=str(root), SHELL="/bin/zsh")
    subprocess.run(
        [str(BINARY), "--config", str(root / "state.yaml"), "add", str(source)],
        env=env, capture_output=True, check=True,
    )
    run_interactive(root, [
        ("type:", b"\x1b[B\x1b[B\r"),
        ("note:", b"Bump deps\r"),
    ], command=("prepare",))
    assert (root / "selected").read_text() == "patch/Bump deps"
    run_interactive(root, [("note:", b"New feature\r")], command=("prepare", "minor"))
    assert (root / "selected").read_text() == "minor/New feature"

print("Mixed enum and free-text arguments: full and partial prompting passed")

with tempfile.TemporaryDirectory(prefix="qrl-rm-test-") as directory:
    root = Path(directory)
    source = root / "keys.toml"
    source.write_text('[x.y]\nz = "./z"\n[x]\nkeep = "./keep"\n')
    env = dict(os.environ, HOME=str(root), ZDOTDIR=str(root), SHELL="/bin/zsh")
    subprocess.run(
        [str(BINARY), "--config", str(root / "state.yaml"), "add", str(source)],
        env=env, capture_output=True, check=True,
    )
    before = (root / "state.yaml").read_bytes()
    run_interactive(root, [("Choose the key or namespace to remove", b"\x1b")],
                    success=False, command=("rm",))
    assert (root / "state.yaml").read_bytes() == before
    # Sorted options are x, x keep, x y, x y z; choose the namespace x y.
    transcript = run_interactive(root, [("x y z", b"\x1b[B\x1b[B\r")], command=("rm",))
    assert b"Removed x y" in transcript
    result = subprocess.run(
        [str(BINARY), "--config", str(root / "state.yaml"), "reload"],
        env=env, capture_output=True, check=True,
    )
    state = (root / "state.yaml").read_text()
    assert "./keep" in state and "./z" not in state
    assert source.read_text() == '[x.y]\nz = "./z"\n[x]\nkeep = "./keep"\n'

print("Key removal: namespace selection, cancellation, and reload persistence passed")

with tempfile.TemporaryDirectory(prefix="qrl-resource-enum-test-") as directory:
    root = Path(directory)
    (root / "second").mkdir()
    (root / "second" / "note.txt").write_text("hello")
    source = root / "resources.toml"
    source.write_text('''
[files.read]
url = './{repo}/{file}'
filehook = 'printf "%s" $file > selected'
args = [
    { name = "repo", enum = ["first", "second"] },
    { name = "file" },
]
[dirs.open]
url = './{repo}'
dirhook = 'cd $dir && pwd > ../selected'
args = [{ name = "repo", enum = ["first", "second"] }]
'''.replace('> selected', f'> "{root / "selected"}"'))
    browser = root / "browser"
    browser.write_text(f'#!/bin/sh\nprintf "%s" "$1" > "{root / "opened"}"\n')
    browser.chmod(0o755)
    source.write_text(source.read_text().replace("./{repo}", str(root) + "/{repo}") + f'''
[links.open]
url = "https://example.com/{{repo}}"
browser = {json.dumps(str(browser))}
args = [{{ name = "repo", enum = ["first", "second repo"] }}]
''')
    env = dict(os.environ, HOME=str(root), ZDOTDIR=str(root), SHELL="/bin/zsh")
    subprocess.run(
        [str(BINARY), "--config", str(root / "state.yaml"), "add", str(source)],
        env=env, capture_output=True, check=True,
    )
    run_interactive(root, [("repo:", b"\x1b[B\r"), ("file:", b"note.txt\r")],
                    command=("files", "read"))
    assert (root / "selected").read_text() == str((root / "second/note.txt").resolve())
    run_interactive(root, [("file:", b"note.txt\r")], command=("files", "read", "second"))
    run_interactive(root, [("repo:", b"\x1b[B\r")], command=("dirs", "open"), directory_channel=True)
    assert (root / "selected").read_text().strip() == str((root / "second").resolve())
    (root / "selected").unlink()
    run_interactive(root, [("repo:", b"\x1b")], success=False, command=("files", "read"))
    assert not (root / "selected").exists()
    transcript = run_interactive(root, [("repo:", b"\x1b[B\r")], command=("links", "open"))
    assert b"qrlkit: open https://example.com/second%20repo" in transcript
    # A detached browser may receive SIGHUP when the PTY session ends.
    subprocess.run(
        [str(BINARY), "--config", str(root / "state.yaml"), "links", "open", "second repo"],
        env=env, capture_output=True, check=True,
    )
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if (root / "opened").exists() and (root / "opened").read_text() == "https://example.com/second%20repo":
            break
        time.sleep(0.01)
    assert (root / "opened").read_text() == "https://example.com/second%20repo"

print("Resource enums: selection, text input, partial arguments, and cancellation passed")

with tempfile.TemporaryDirectory(prefix="qrl-theme-test-") as directory:
    root = Path(directory)
    target = root / "repo.txt"
    target.write_text("repo")
    source = root / "resources.toml"
    source.write_text(f'''theme = {{ border = "rounded", border_color = "#9399b2" }}
[gh]
theme = {{ selected = "#cba6f7", text = "#cdd6f4", hint = "#9399b2", marker = "--x", border = "double" }}
home = "{target}"
[gh.task]
args = [{{ name = "Color", enum = ["blue", "red"] }}, {{ name = "Name" }}]
run = 'printf "%s %s\\n" "$1" "$2"'
[qk]
theme = "ocean"
docs = "{target}"
''')
    env = dict(os.environ, HOME=str(root), ZDOTDIR=str(root), SHELL="/bin/zsh")
    subprocess.run([str(BINARY), "--config", str(root / "state.yaml"), "add", str(source)], env=env, capture_output=True, check=True)
    transcript = run_interactive(root, [("--x gh", b"\r"), ("--x home", b"\r")], command=(), colors=True)
    assert "╭".encode() in transcript and "╔".encode() in transcript, transcript
    assert b"38;2;203;166;247" in transcript, transcript
    assert b"38;2;136;192;208" in transcript, transcript
    assert str(target.resolve()).encode() in transcript
    transcript = run_interactive(root, [("Color:", b"\r"), ("Name:", b"friend\r")], command=("gh", "task"), colors=True)
    assert "╔".encode() in transcript and b"--x" in transcript, transcript
    assert b"blue friend" in transcript, transcript
    run_interactive(root, [("--x home", b"\x1b")], success=False, command=("gh",))
    source.write_text(source.read_text().replace('marker = "--x"', 'marker = "->"'))
    run_interactive(root, [("-> home", b"\r")], command=("gh",))

    source.write_text(source.read_text() + f'\n[plain]\nhome = "{target}"\n')
    subprocess.run([str(BINARY), "--config", str(root / "state.yaml"), "themes", "set", "forest"], env=env, capture_output=True, check=True)
    transcript = run_interactive(root, [("home", b"\r")], command=("plain",), colors=True)
    assert b"38;2;163;190;140" in transcript, transcript
    transcript = run_interactive(root, [("-> home", b"\r")], command=("gh",), colors=True)
    assert b"38;2;203;166;247" in transcript, transcript
    # The custom selection stays purple; the unspecified title inherits forest.
    assert b"38;2;163;190;140" in transcript, transcript
    transcript = run_interactive(root, [("docs", b"\r")], command=("qk",), colors=True)
    assert b"38;2;136;192;208" in transcript, transcript
    transcript = run_interactive(root, [("Choose the key or namespace", b"\x1b")], success=False, command=("rm",), colors=True)
    assert b"38;2;163;190;140" in transcript, transcript
    subprocess.run([str(BINARY), "--config", str(root / "state.yaml"), "themes", "set", "terminal"], env=env, capture_output=True, check=True)
    transcript = run_interactive(root, [("home", b"\r")], command=("plain",), colors=True)
    assert b"38;2;163;190;140" not in transcript, transcript

print("Namespace themes: colors, borders, markers, argument prompts, cancellation, and automatic reload passed")
print("Default themes: persisted selection, namespace overrides, general prompts, and reset passed")
