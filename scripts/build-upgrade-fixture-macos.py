"""Build a labeled install-test payload; never include it in release artifacts."""

import json
import os
from pathlib import Path
import plistlib
import sys
import tempfile

from release_macos import (
    AGENT_PATH, HOST_BASE, component_package, host_postinstall, host_preinstall,
    launch_agent, normalize_modes, payload_digest, run, write_json,
)


def main():
    if sys.platform != "darwin" or os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("Upgrade fixtures are restricted to disposable macOS CI runners")
    source, output = map(Path, sys.argv[1:])
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="plait-upgrade-fixture-") as directory:
        work = Path(directory)
        expanded = work / "expanded"
        run("pkgutil", "--expand-full", source, expanded)
        payload = expanded / "Payload"
        manifests = list(payload.rglob("component.json"))
        if len(manifests) != 1:
            raise ValueError("Expected one host manifest")
        metadata = json.loads(manifests[0].read_text())
        if metadata["component"] != "host":
            raise ValueError("Expected host package")
        old = manifests[0].parent
        manifests[0].unlink()
        (old / "INSTALLATION_TEST_FIXTURE.txt").write_text(
            "DISPOSABLE CI UPGRADE FIXTURE. Executable code is unchanged. Not a release.\n"
        )
        normalize_modes(old)
        metadata["payload_sha256"] = payload_digest(old)
        metadata["build_id"] = metadata["version"] + "-" + metadata["payload_sha256"][:20]
        metadata["fixture"] = "disposable upgrade simulation, not a release"
        installed = HOST_BASE / "releases" / metadata["build_id"]
        target = payload / installed.relative_to("/")
        old.rename(target)
        write_json(target / "component.json", metadata)
        write_json(payload / HOST_BASE.relative_to("/") / "installed.json", metadata)
        (payload / AGENT_PATH.relative_to("/")).write_bytes(plistlib.dumps(launch_agent(installed)))
        scripts = work / "scripts"
        scripts.mkdir()
        for name, content in [("preinstall", host_preinstall(installed)), ("postinstall", host_postinstall(installed))]:
            (scripts / name).write_text(content)
            (scripts / name).chmod(0o755)
        component_package(payload, scripts, "host", metadata["version"], output, work)


if __name__ == "__main__":
    main()
