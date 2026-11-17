from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class PrebuiltInstallTests(unittest.TestCase):
    def run_install(self, *, busy=False, signed=True):
        with tempfile.TemporaryDirectory(prefix="plait-prebuilt-test-") as directory:
            root = Path(directory)
            bundle = root / "download with spaces/Plait.app"
            resources = bundle / "Contents/Resources"
            resources.mkdir(parents=True)
            (resources / "host.json").write_text("{}")
            script = (ROOT / "scripts/update-installed-macos.sh").read_text()
            replacements = {
                "/usr/bin/uname": "fixture_uname",
                "/usr/bin/sw_vers": "fixture_version",
                "/usr/bin/codesign": "fixture_signature",
                "/usr/bin/open": "fixture_open",
            }
            for original, replacement in replacements.items():
                script = script.replace(original, replacement)
            path = root / "installer.sh"
            path.write_text(script)
            calls = root / "calls"
            driver = f'''
source {shlex.quote(str(path))}
record() {{ printf '%s\\n' "$*" >> {shlex.quote(str(calls))}; }}
fixture_uname() {{ if [[ $1 == -s ]]; then echo Darwin; else echo arm64; fi; }}
fixture_version() {{ echo 15.0; }}
fixture_signature() {{ return {0 if signed else 1}; }}
field() {{ echo dev.plait.desktop; }}
active_runs() {{ echo {1 if busy else 0}; }}
quit_client() {{ record quit; }}
migrate_legacy() {{ record migrate; }}
install_app() {{ record "app:$1"; }}
install_host() {{ record host; }}
activate_host() {{ record activate; }}
fixture_open() {{ record open; }}
bash() {{ record unexpected-build; return 97; }}
main --app {shlex.quote(str(bundle))}
'''
            result = subprocess.run(["/bin/bash"], input=driver, text=True, capture_output=True)
            events = calls.read_text().splitlines() if calls.exists() else []
            return result, events, str(bundle)

    def test_prebuilt_app_needs_no_checkout_or_build_tools(self):
        result, events, bundle = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(events, ["quit", "migrate", f"app:{bundle}", "host", "activate", "open"])

    def test_active_run_refuses_before_quitting_or_replacing_anything(self):
        result, events, _ = self.run_install(busy=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(events, [])
        self.assertIn("1 run(s)", result.stdout)

    def test_invalid_app_refuses_before_removing_legacy_installation(self):
        result, events, _ = self.run_install(signed=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(events, [])


if __name__ == "__main__":
    unittest.main()
