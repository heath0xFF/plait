# Releasing Plait

Installed per-user apps and headless hosts update from GitHub Releases. A
release is a pushed tag; the macOS workflow builds, tests, signs the update
manifest, and publishes.

## Publish a preview

1. Keep the Cargo version for `plait` and `plait-host` the same
   (`MAJOR.MINOR.PATCH`). A preview tag adds `-preview.N`, counting from 1.
2. Get the release commit ready on a branch. The tests for what changed pass,
   and a change that writes a person's files, or a file an older build reads,
   has been read by someone who did not write it. A release is built and
   checked once, by its tag's run: a full local `scripts/check.sh` before
   tagging is not required, and repeats what that run does. Render the scenes
   and measure frame cost only when the change touches drawing.
3. Tag that commit and push the tag alone.

   ```sh
   git tag v0.1.0-preview.7
   git push origin v0.1.0-preview.7
   ```

4. The **macOS build** workflow builds from that tag. Its check job (format,
   lint, tests, and the real command sandbox) and package job run side by
   side; the package job builds the installers and runs the installer smoke
   test, including migration to the prebuilt per-user install. Its publish job
   writes `update.json`, signs it, and publishes the release with the app
   archive, host archive, packages, DMG, and checksums. The
   [native acceptance check](VERIFICATION.md#native-acceptance-check) drives
   the same package in a job of its own; it is advisory, and publishing does
   not wait for it.
5. When the release is published, push the commit to `main`. That push does
   not build or check on macOS again, because the tag's run did both for this
   commit; the Linux check runs. If the tag's run fails, nothing is published
   and the tag stays as it is: fix the branch and tag the next number.

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
