# macOS development package

## Build

Use an Apple Silicon Mac with Git, rustup, Python 3, and full Xcode with the
Metal compiler. Run from a Git checkout:

```sh
bash scripts/build-macos.sh
open dist/Plait.app --args /absolute/path/to/repository
```

The script builds the locked release binary, installs pinned `cargo-about`
0.9.3 under `target/build-tools`, and generates dependency notices. The first
build needs network access for tools, dependencies, and license data.

Output:

- `dist/Plait.app`: native development app with the Plait Dock/Finder icon.
- `dist/Plait-<version>-<revision>-macos-arm64.zip`: portable bundle archive.
- Matching `.sha256` file: checks for archive corruption, not publisher identity.

The app version comes from Cargo. The bundle records the Git revision and adds
`-dirty` when the checkout has changes. A rebuild replaces the generated app
and the archive with the same name. It does not install or replace anything
in `/Applications`.

Packaging checks the executable architecture, system-only dynamic links,
license notices, icon, property list, and ad-hoc signature. It extracts the ZIP
and repeats the bundle checks before publishing the output. Assets come from
the [supplied vector kit](../assets/branding/README.md). No Node runtime, model
credentials, user data, or Pi configuration is packaged.

## GitHub builds

`.github/workflows/macos.yml` checks Rust, runs the offline Pi Durable recovery
probe, and builds an Apple Silicon development package on pull requests and
`main`. Download the ZIP and checksum from the run's `plait-macos-arm64-…`
artifact. Artifacts expire after 14 days. This workflow does not publish releases
or use signing secrets. Native accessibility smoke tests still run locally.

## Development only

The bundle declares macOS 15 or later and is built for Apple Silicon only.
Local UI verification is on macOS 26.6.2; older-system runtime coverage is not
yet complete.

This build is **ad-hoc signed, not Developer ID signed or notarized**. A downloaded
copy may be blocked by Gatekeeper. Do not disable Gatekeeper. Build from source
or use macOS's explicit per-app approval only after checking the source and
trusting the artifact. There is no installer or automatic updater.

[Issue #6](https://github.com/heath0xFF/plait/issues/6) tracks signed releases,
notarization, and automatic updates. Those require a Developer ID Application
identity, protected signing keys, and verified update delivery. An Apple
Development identity is not a substitute for public distribution.

This package still has fixture conversations and a disabled composer. Packaging
does not connect the host or enable Plait to run coding tasks. M2 must connect
the independent host, model selection, and composer before Plait can build itself.
