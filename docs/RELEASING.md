# Releasing Plait

Installed per-user apps and headless hosts update from GitHub Releases. A
release is a pushed tag; the macOS workflow builds, tests, signs the update
manifest, and publishes.

## Publish a preview

1. Keep the Cargo version for `plait` and `plait-host` the same
   (`MAJOR.MINOR.PATCH`). A preview tag adds `-preview.N`, counting from 1.
2. Merge to `main` with local checks passing (`scripts/check.sh`).
3. Tag the merged commit and push the tag. There is no need to wait for
   `main`'s macOS build first: the tag's build runs the same checks on the same
   commit and publishes nothing unless they pass.

   ```sh
   git tag v0.1.0-preview.7
   git push origin v0.1.0-preview.7
   ```

4. The **macOS build** workflow builds from that tag. Its check job (format,
   lint, tests, and the real command sandbox) and package job run side by side;
   the package job builds the installers and runs the installer
   smoke test, including migration to the prebuilt per-user install. Its publish job writes `update.json`, signs it, and publishes the
   release with the app archive, host archive, packages, DMG, and checksums.

Installed apps see the release on their next launch or **Check for Updates…**.
The updater compares the signed version, so each release needs a newer tag.
Published tags and assets are not rewritten.

## Update signing key

Plait accepts only manifests signed by the key whose public half is
`PUBLIC_KEY` in `crates/plait-core/src/update.rs`. The private half is the
Actions secret **`PLAIT_UPDATE_SIGNING_KEY`**: 64 hex digits. The publish job
fails if the secret is missing or does not match the built-in public key.

The maintainer keeps a local copy outside Git with mode 0600. Back it up,
encrypted, off this computer: GitHub cannot return a secret's value, and losing
the key strands installed apps on their current version until they are
reinstalled with a build that trusts a new key. Do not generate a key per release
or sign test builds with it.

Create a key only when none exists, then store it as the secret:

```sh
cargo run -p plait-core --example update-key -- generate <private-key-file>
gh secret set PLAIT_UPDATE_SIGNING_KEY < <private-key-file>
```

`generate` refuses to overwrite a file and prints the public key to put in
`update.rs`. `update-key verify <dir>/update.json` checks a downloaded manifest
against the built-in key.

## What a signature covers

The manifest records the version, tag, revision, host build identity, and each
artifact's name, size, and SHA-256. Clients verify the signature before reading
any field, reject a tag that differs from the release they asked for, and
discard downloads whose size or digest differ. Workflow changes that can read
the signing secret can sign updates; review them like release code.
