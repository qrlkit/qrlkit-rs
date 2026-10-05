#!/usr/bin/env python3
"""Test installation with local release fixtures; never access the network."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().parents[1] / "install.sh"
RELEASES = "https://github.com/qrlkit/qrlkit-rs/releases"


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.destination = self.root / "installed tools"
        self.env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}",
                        HOME=str(self.root), QRLKIT_INSTALL_DIR=str(self.destination),
                        SHELL="/bin/bash", ZDOTDIR="", XDG_CONFIG_HOME="",
                        QRLKIT_NO_MODIFY_PATH="0",
                        FIXTURES=str(self.root), TEST_OS="Linux", TEST_ARCH="x86_64")
        self.stub("uname", '#!/bin/sh\ncase "$1" in -s) echo "$TEST_OS";; -m) echo "$TEST_ARCH";; esac\n')
        self.stub("curl", '''#!/bin/sh
set -eu
output=
for arg in "$@"; do
    if [ "${previous:-}" = --output ]; then output=$arg; fi
    previous=$arg
    url=$arg
done
printf '%s\\n' "$url" >> "$FIXTURES/requests"
case "$url" in
    https://github.com/qrlkit/qrlkit-rs/releases/latest)
        printf '%s' https://github.com/qrlkit/qrlkit-rs/releases/tag/v1.2.3 ;;
    https://github.com/qrlkit/qrlkit-rs/releases/download/v1.2.3/*)
        cp "$FIXTURES/${url##*/}" "$output" ;;
    *) exit 22 ;;
esac
''')

    def stub(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o755)

    def fixture(self, platform="linux-x86_64", corrupt=False, missing=False):
        directory = f"qrlkit-1.2.3-{platform}"
        archive = self.root / f"{directory}.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            content = b"#!/bin/sh\necho 'qrlkit 1.2.3'\n"
            entry = tarfile.TarInfo(f"{directory}/{'other' if missing else 'qrlkit'}")
            entry.size = len(content)
            entry.mode = 0o755
            tar.addfile(entry, io.BytesIO(content))
        digest = "0" * 64 if corrupt else hashlib.sha256(archive.read_bytes()).hexdigest()
        Path(f"{archive}.sha256").write_text(f"{digest}  {archive.name}\n")

    def run_installer(self, *args):
        # stdin also exercises the documented curl | sh installation method.
        return subprocess.run(["sh", "-s", "--", *args], input=INSTALLER.read_text(),
                              env=self.env, cwd=self.root, capture_output=True, text=True)

    def test_latest_on_supported_platforms(self):
        for system, machine, platform in [
            ("Linux", "x86_64", "linux-x86_64"),
            ("Linux", "aarch64", "linux-aarch64"),
            ("Darwin", "x86_64", "macos-x86_64"),
            ("Darwin", "arm64", "macos-aarch64"),
        ]:
            with self.subTest(platform=platform):
                self.env.update(TEST_OS=system, TEST_ARCH=machine)
                self.fixture(platform)
                result = self.run_installer()
                self.assertEqual(result.returncode, 0, result.stderr)
                installed = subprocess.check_output([str(self.destination / "qrlkit")], text=True)
                self.assertEqual(installed.strip(), "qrlkit 1.2.3")
                self.assertIn(f"{RELEASES}/download/v1.2.3/qrlkit-1.2.3-{platform}.tar.gz",
                              (self.root / "requests").read_text())
        self.assertIn(f"{RELEASES}/latest", (self.root / "requests").read_text())

    def test_latest_and_default_destination(self):
        self.fixture()
        del self.env["QRLKIT_INSTALL_DIR"]
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / ".local/bin/qrlkit").is_file())
        self.assertIn("PATH", result.stdout)
        self.assertIn(f"{RELEASES}/latest", (self.root / "requests").read_text())

    def test_failed_download_or_verification_preserves_existing_install(self):
        self.destination.mkdir()
        binary = self.destination / "qrlkit"
        binary.write_text("existing binary")
        for failure in ("download", "checksum", "missing binary"):
            with self.subTest(failure=failure):
                if failure != "download":
                    self.fixture(corrupt=failure == "checksum", missing=failure == "missing binary")
                result = self.run_installer()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(binary.read_text(), "existing binary")
                self.assertEqual(list(self.destination.iterdir()), [binary])

    def test_rejects_unsupported_platform(self):
        for updates in [{"TEST_OS": "Windows"}, {"TEST_ARCH": "i686"}]:
            with self.subTest(updates=updates):
                self.env.update(TEST_OS="Linux", TEST_ARCH="x86_64")
                self.env.update(updates)
                result = self.run_installer()
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.destination.exists())
                self.assertFalse((self.root / "requests").exists())

    def test_rejects_arguments(self):
        result = self.run_installer("1.2.3")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Usage:", result.stderr)
        self.assertFalse((self.root / "requests").exists())

    def test_help(self):
        result = self.run_installer("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Usage:", result.stdout)
        self.assertFalse((self.root / "requests").exists())

    def test_shell_startup_selection_and_repeat_install(self):
        cases = [
            ("zsh", "Linux", {}, ".zshrc"),
            ("zsh", "Darwin", {"ZDOTDIR": str(self.root / "zsh")}, "zsh/.zshrc"),
            ("bash", "Linux", {}, ".bashrc"),
            ("bash", "Darwin", {}, ".bash_profile"),
            ("fish", "Linux", {}, ".config/fish/config.fish"),
            ("fish", "Linux", {"XDG_CONFIG_HOME": str(self.root / "config")}, "config/fish/config.fish"),
        ]
        for shell, system, overrides, profile in cases:
            with self.subTest(shell=shell, system=system, profile=profile):
                self.env.update(SHELL=f"/bin/{shell}", TEST_OS=system,
                                ZDOTDIR="", XDG_CONFIG_HOME="")
                self.env.update(overrides)
                self.fixture("macos-x86_64" if system == "Darwin" else "linux-x86_64")
                startup = self.root / profile
                startup.parent.mkdir(parents=True, exist_ok=True)
                startup.write_text("# existing user configuration")
                for _ in range(2):
                    result = self.run_installer()
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("Restart your terminal", result.stdout)
                content = startup.read_text()
                self.assertTrue(content.startswith("# existing user configuration\n"))
                self.assertEqual(content.count("# qrlkit PATH"), 1)
                self.assertIn("fish_add_path" if shell == "fish" else "export PATH", content)

    def test_macos_bash_preserves_existing_login_profile(self):
        self.env["TEST_OS"] = "Darwin"
        self.fixture("macos-x86_64")
        profile = self.root / ".profile"
        profile.write_text("# login settings\n")
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("# qrlkit PATH", profile.read_text())
        self.assertFalse((self.root / ".bash_profile").exists())

    def test_path_with_shell_metacharacters_is_literal_and_not_duplicated(self):
        self.fixture()
        destination = self.root / "bin ' quoted \\ $(touch unexpected) $HOME `echo hi`"
        self.env["QRLKIT_INSTALL_DIR"] = str(destination)
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run(
            ["sh", "-c", '. "$HOME/.bashrc"; . "$HOME/.bashrc"; printf "%s" "$PATH"'],
            env=self.env, cwd=self.root, capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"{destination}:{self.env['PATH']}")
        self.assertFalse((self.root / "unexpected").exists())

    def test_path_setup_skipped_when_unnecessary_disabled_or_unknown(self):
        self.fixture()
        for overrides in [
            {"PATH": f"{self.destination}:{self.env['PATH']}"},
            {"QRLKIT_NO_MODIFY_PATH": "1"},
            {"SHELL": "/bin/unknown"},
            {"SHELL": ""},
        ]:
            with self.subTest(overrides=overrides):
                original = self.env.copy()
                self.env.update(overrides)
                result = self.run_installer()
                self.env = original
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertFalse((self.root / ".bashrc").exists())


if __name__ == "__main__":
    unittest.main()
