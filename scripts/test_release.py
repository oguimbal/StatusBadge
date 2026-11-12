"""Exercise the release script in a temporary Git repo with fake external tools."""
import os
import pathlib
import shutil
import subprocess
import tempfile
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="statusbadge-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)
        for name in ("scripts/release.sh", "bundle/verify-version.py", "bundle/Info.plist", "Cargo.toml", "Cargo.lock", ".gitignore"):
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, target)
        self.bin = self.root / "fake-tools"
        self.bin.mkdir()
        self.log = self.root / "calls.log"
        # RELEASE_REMOTE: the fixtures push to a local bare repository, not to github.com.
        self.env = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}",
                    "RELEASE_TEST_LOG": str(self.log), "RELEASE_REMOTE": "origin", "FAIL_AT": ""}
        with (self.root / ".gitignore").open("a") as ignore:
            ignore.write("\nfake-tools/\ncalls.log\n")
        # Signing and packaging tools must never be reached: they fail the release if called.
        for name in ("cargo", "codesign", "xcrun", "create-dmg", "security"):
            self.script(self.bin / name, name, ":" if name == "cargo" else "exit 1")
        (self.root / "scripts/test_fixture.py").write_text(
            "import unittest\n"
            "class ReleaseCheck(unittest.TestCase):\n"
            "    def test_runs(self): pass\n"
        )
        self.git("init", "-b", "main")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Release Test")
        self.git("add", ".")
        self.git("commit", "-m", "fixture")

    def script(self, path, name, body):
        path.write_text(f'#!/bin/bash\nset -eu\nprintf "%s %s\\n" "{name}" "$*" >> "$RELEASE_TEST_LOG"\n'
                        f'if [ "$FAIL_AT" = "{name} $1" ]; then exit 1; fi\n{body}\n')
        path.chmod(0o755)

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True, stderr=subprocess.DEVNULL).strip()

    def run_release(self, *args, **env):
        return subprocess.run(["bash", "scripts/release.sh", *args], cwd=self.root,
                              env={**self.env, **env}, capture_output=True, text=True)

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def version(self):
        return tomllib.loads((self.root / "Cargo.toml").read_text())["workspace"]["package"]["version"]

    def add_remote(self, name="origin"):
        directory = tempfile.TemporaryDirectory(prefix="statusbadge-release-remote-")
        self.addCleanup(directory.cleanup)
        remote = pathlib.Path(directory.name)
        subprocess.run(["git", "init", "--bare", str(remote)], check=True, capture_output=True)
        self.git("remote", "add", name, str(remote))
        self.git("push", "-u", name, "main")
        return remote

    def test_default_command_increments_latest_tag_and_pushes(self):
        remote = self.add_remote()
        major, minor, patch = map(int, self.version().split('.'))
        previous = f"v{major}.{minor}.{patch + 10}"
        self.git("tag", previous)
        self.git("push", "origin", previous)
        self.git("tag", "-d", previous)  # Only the remote knows the latest release.
        result = self.run_release()
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = f"{major}.{minor}.{patch + 11}"
        self.assertEqual(self.version(), expected)
        self.assertIn(f"<string>{expected}</string>", (self.root / "bundle/Info.plist").read_text())
        remote_commit = subprocess.check_output(["git", "--git-dir", str(remote), "rev-parse", f"v{expected}^{{commit}}"], text=True).strip()
        self.assertEqual(remote_commit, self.git("rev-parse", "HEAD"))
        self.assertEqual(self.git("log", "-1", "--format=%s"), f"release v{expected}")
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_nothing_is_built_signed_or_notarized_locally(self):
        self.add_remote()
        result = self.run_release()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({line.split()[0] for line in self.calls()}, {"cargo"})
        self.assertFalse(any(line.startswith("cargo build") for line in self.calls()))
        self.assertFalse((self.root / "dist").exists())

    def test_first_release_tags_the_current_version_without_a_commit(self):
        head = self.git("rev-parse", "HEAD")
        result = self.run_release("current", "--no-push")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git("tag", "--list"), f"v{self.version()}")
        self.assertEqual(self.git("rev-parse", "HEAD"), head)

    def test_failed_check_prevents_commit_and_tag(self):
        head = self.git("rev-parse", "HEAD")
        result = self.run_release("patch", "--no-push", FAIL_AT="cargo test")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.git("tag", "--list"), "")
        self.assertEqual(self.git("rev-parse", "HEAD"), head)

    def test_rejected_git_push_fails(self):
        remote = self.add_remote()
        hook = remote / "hooks/pre-receive"
        hook.write_text("#!/bin/sh\nexit 1\n")
        hook.chmod(0o755)
        result = self.run_release()
        self.assertNotEqual(result.returncode, 0)
        tags = subprocess.check_output(["git", "--git-dir", str(remote), "tag", "--list"], text=True)
        self.assertEqual(tags, "")

    def test_release_goes_to_the_github_remote_only(self):
        self.add_remote("gitea")
        result = self.run_release(RELEASE_REMOTE="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("github.com", result.stderr)
        self.assertEqual(self.calls(), [])
        self.assertEqual(self.git("tag", "--list"), "")
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_dirty_tree_is_refused(self):
        (self.root / "Cargo.toml").write_text((self.root / "Cargo.toml").read_text() + "\n")
        result = self.run_release("patch", "--no-push")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
