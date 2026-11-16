"""Build a Finder-friendly disk image and the command-line installer's release index."""

import json
import os
from pathlib import Path
import re
import shutil

from release_macos import digest, run, write_json

ROOT = Path(__file__).resolve().parent.parent


def release_index(component_release, tag):
    version = component_release["client"]["version"]
    if not re.fullmatch(r"v" + re.escape(version) + r"(?:-preview\.[0-9]+)?", tag):
        raise ValueError("Release tag must match the packaged client version")
    revision = component_release["revision"]
    if not re.fullmatch(r"[0-9a-f]{12,40}", revision):
        raise ValueError("Only clean source builds can have a release index")
    downloads = {}
    for component, prefix in [("all", "Installer"), ("client", "Client"), ("host", "Host")]:
        matches = [item for item in component_release["artifacts"] if item["name"].startswith(f"Plait-{prefix}-")]
        if len(matches) != 1:
            raise ValueError("Expected exactly one package for each component")
        item = matches[0]
        if not re.fullmatch(r"Plait-(Installer|Client|Host)-[0-9a-fA-Z.-]+-macos-arm64\.pkg", item["name"]):
            raise ValueError("Invalid package filename")
        if not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]):
            raise ValueError("Invalid package checksum")
        downloads[component] = dict(item)
    build_id = component_release["host"]["build_id"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+-[0-9a-f]{20}", build_id):
        raise ValueError("Invalid host build identity")
    return {
        "schema_version": 1, "tag": tag, "revision": revision,
        "signing": "unsigned-preview", "host_build_id": build_id, "downloads": downloads,
    }


def build_distribution(installers, version, revision, work):
    image_root = work / "disk-image"
    image_root.mkdir()
    for prefix, name in [("Installer", "Install Plait.pkg"), ("Client", "Update Client.pkg")]:
        package = next(path for path in installers if path.name.startswith(f"Plait-{prefix}-"))
        shutil.copy2(package, image_root / name)
    (image_root / "Read me first.txt").write_text(
        "PLAIT — LOCAL PREVIEW\n\n"
        "First installation: quit Plait and open Install Plait.pkg.\n"
        "The package installs the native app and an independent per-user host.\n"
        "If this exact host build is already installed, use Update Client.pkg.\n\n"
        "These packages are unsigned and not notarized. Use macOS's explicit\n"
        "per-item approval only if you trust this release. Do not disable Gatekeeper.\n\n"
        "Open Plait from Applications, open a repository, and choose an authenticated\n"
        "model in Settings. The host uses your existing Pi account.\n"
        "Model requests need internet access. Project build tools are not bundled.\n\n"
        "Client updates do not stop host work. Host updates are staged; finish work,\n"
        "then log out and back in to activate them. Do not delete old host releases.\n\n"
        "Setup, authentication, and updates:\n"
        "https://github.com/heath0xFF/plait/blob/main/docs/MACOS.md\n"
    )
    image = work / f"Plait-{version}-{revision}-macos-arm64.dmg"
    run("hdiutil", "create", "-volname", "Plait", "-srcfolder", image_root,
        "-fs", "HFS+", "-format", "UDZO", image)
    run("hdiutil", "verify", image)
    mount = work / "mounted-image"
    mount.mkdir()
    run("hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", mount, image)
    try:
        for source in image_root.iterdir():
            if digest(source) != digest(mount / source.name):
                raise ValueError("Disk image contents differ from the verified installer")
    finally:
        run("hdiutil", "detach", mount)
    script = work / "install.sh"
    shutil.copy2(ROOT / "scripts/install.sh", script)
    artifacts = [image, script]
    if not revision.endswith("-dirty"):
        component = next(path for path in installers if path.name.startswith("Plait-Release-"))
        tag = os.environ.get("PLAIT_RELEASE_TAG") or f"v{version}-preview.0"
        index = work / "release.json"
        write_json(index, release_index(json.loads(component.read_text()), tag))
        artifacts.append(index)
    return artifacts
