import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from distribution_macos import release_index

ROOT = Path(__file__).resolve().parent.parent


def component_release():
    return {
        "revision": "a" * 12, "client": {"version": "0.1.0"},
        "host": {"build_id": "0.1.0-" + "b" * 20},
        "artifacts": [
            {"name": f"Plait-{name}-0.1.0-{'a' * 12}-macos-arm64.pkg", "sha256": hashlib.sha256(b"fixture package").hexdigest()}
            for name in ["Client", "Host", "Installer"]
        ],
    }


class DistributionTests(unittest.TestCase):
    def test_index_matches_the_packaged_version_and_all_components(self):
        value = release_index(component_release(), "v0.1.0-preview.1")
        self.assertEqual(set(value["downloads"]), {"all", "client", "host"})
        self.assertEqual(value["signing"], "unsigned-preview")
        self.assertEqual(value["tag"], "v0.1.0-preview.1")
        release_index(component_release(), "v0.1.0")

    def test_rejects_dirty_builds_bad_tags_and_unsafe_artifact_metadata(self):
        for tag in ["latest", "../escape", "v9.0.0", "v0.1.0;echo bad"]:
            with self.assertRaises(ValueError):
                release_index(component_release(), tag)
        for field, value in [("revision", "a" * 12 + "-dirty"), ("artifacts", [])]:
            fixture = component_release()
            fixture[field] = value
            with self.assertRaises(ValueError):
                release_index(fixture, "v0.1.0")
        for key, value in [("name", "Plait-Client-../../escape.pkg"), ("sha256", "not a checksum")]:
            fixture = component_release()
            fixture["artifacts"][0][key] = value
            with self.assertRaises(ValueError):
                release_index(fixture, "v0.1.0")


# Only this private test copy calls shims. Production has no test-mode or URL override.
SHIM = '''#!/usr/bin/env python3
import json, os, pathlib, sys
root = pathlib.Path(os.environ["PLAIT_INSTALL_FIXTURE"])
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with (root / "calls").open("a") as log: log.write(json.dumps([name, args]) + "\\n")
if name == "uname": print("Darwin" if args == ["-s"] else "arm64")
elif name == "sw_vers": print("26.0")
elif name == "plutil":
    value = json.loads(pathlib.Path(args[-1]).read_text())
    for part in args[args.index("-extract") + 1].split("."):
        value = value[int(part)] if isinstance(value, list) else value[part]
    print(value)
elif name == "curl":
    url = next(value for value in args if value.startswith("https://"))
    output = pathlib.Path(args[args.index("--output") + 1])
    if "api.github.com" in url: output.write_text('[{"tag_name":"v0.1.0-preview.1"}]')
    elif url.endswith("release.json"): output.write_bytes((root / "release.json").read_bytes())
    elif url.endswith(".pkg"): output.write_bytes(b"fixture package")
    else: sys.exit(22)
elif name == "sudo": pass
else: sys.exit(1)
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="plait-installer-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.metadata = release_index(component_release(), "v0.1.0-preview.1")
        self.script = (ROOT / "scripts/install.sh").read_text()
        for command in ["uname", "sw_vers", "plutil", "curl", "sudo"]:
            shim = self.root / command
            shim.write_text(SHIM)
            shim.chmod(0o755)
            self.script = self.script.replace(f"/usr/bin/{command}", str(shim))
        self.host = self.root / "host"
        self.script = self.script.replace("host_base='/Library/Application Support/Plait/Host'", f"host_base='{self.host}'")

    def run_installer(self, *args):
        (self.root / "release.json").write_text(json.dumps(self.metadata))
        script = self.root / "install.sh"
        script.write_text(self.script)
        return subprocess.run(["bash", str(script), *args], capture_output=True, text=True,
                              env={**os.environ, "PLAIT_INSTALL_FIXTURE": str(self.root)})

    def calls(self):
        path = self.root / "calls"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_help_has_no_network_or_system_effect(self):
        self.assertEqual(self.run_installer("--help").returncode, 0)
        self.assertEqual(self.calls(), [])

    def test_unsigned_install_requires_explicit_consent(self):
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--allow-unsigned", result.stderr)
        self.assertFalse(any(name == "sudo" for name, _ in self.calls()))

    def test_verified_download_needs_no_administrator_and_preserves_existing_file(self):
        output = self.root / "downloads with spaces"
        result = self.run_installer("--download", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        package = output / self.metadata["downloads"]["all"]["name"]
        self.assertEqual(package.read_bytes(), b"fixture package")
        package.write_bytes(b"existing data")
        result = self.run_installer("--download", str(output))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(package.read_bytes(), b"existing data")
        self.assertFalse(any(name == "sudo" for name, _ in self.calls()))

    def test_corrupt_or_injected_metadata_never_reaches_installer(self):
        original = copy.deepcopy(self.metadata)
        for field, value in [("sha256", "0" * 64), ("name", "../../outside.pkg"), ("sha256", "$(touch outside)")]:
            self.metadata = copy.deepcopy(original)
            self.metadata["downloads"]["all"][field] = value
            self.assertNotEqual(self.run_installer("--allow-unsigned").returncode, 0)
        self.metadata = copy.deepcopy(original)
        self.metadata["tag"] = "v9.0.0"
        self.assertNotEqual(self.run_installer("--allow-unsigned").returncode, 0)
        self.assertFalse(any(name == "sudo" for name, _ in self.calls()))

    def test_explicit_version_is_validated_without_shell_evaluation(self):
        result = self.run_installer("--version", "../escape; echo bad", "--allow-unsigned")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(name in ["curl", "sudo"] for name, _ in self.calls()))

    def test_same_host_build_selects_client_only_and_never_restarts_host(self):
        (self.host / "releases" / self.metadata["host_build_id"]).mkdir(parents=True)
        result = self.run_installer("--allow-unsigned")
        self.assertEqual(result.returncode, 0, result.stderr)
        install = [args for name, args in self.calls() if name == "sudo"]
        self.assertEqual(len(install), 1)
        self.assertIn(self.metadata["downloads"]["client"]["name"], install[0][2])
        self.assertFalse(any("launchctl" in str(call) for call in self.calls()))
        result = self.run_installer("--component", "host", "--allow-unsigned")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already installed", result.stdout)
        self.assertEqual(len([1 for name, _ in self.calls() if name == "sudo"]), 1)


if __name__ == "__main__":
    unittest.main()
