# Releasing Plait

Installed per-user apps and headless hosts update from GitHub Releases. A
release is made by merging to `main`: GitHub builds what was merged, signs the
update manifest, and publishes the next preview. Nobody tags by hand.

## How a change reaches a release

1. **Work on a branch.** `main` changes only through a pull request; GitHub
   refuses a direct push.
2. **Open a pull request.** GitHub runs the tests on it: the macOS check
   (format, lint, tests, and the real command sandbox) and the Linux check. A
   request that changes only prose skips the macOS check. The request can merge
   when both pass and it is up to date with `main`, so what was tested is what
   is merged. A change that writes a person's files, or a file an older build
   reads, is also read by someone who did not write it before it merges.
3. **Merge.** The push to `main` is not tested again. If `main` now holds code
   that ships and has not been released, the **macOS build** workflow builds it
   once: the package job builds the installers and runs the installer smoke
   test, including migration to the prebuilt per-user install, and the publish
   job writes `update.json`, signs it, and publishes the release with the app
   archive, host archive, packages, DMG, and checksums. The
   [native acceptance check](VERIFICATION.md#native-acceptance-check) drives
   the same package in a job of its own; it is advisory, and publishing does
   not wait for it.

A merge that changes only docs, tests or CI publishes nothing;
`scripts/release_plan.py` holds the list of paths that never reach an
installed app.

## Preview numbers

Keep the Cargo version for `plait` and `plait-host` the same
(`MAJOR.MINOR.PATCH`). A preview is `v<version>-preview.N`. The workflow takes
the next `N` after the highest tag for that version, and creates the tag with
the release, on the commit it built, only when the build and the installer
test have passed. A build that fails uses up no number: fix it with another
pull request.

Installed apps see the release on their next launch or **Check for Updates…**.
The updater compares the signed version, so each release needs a newer tag.
Published tags and assets are not rewritten. A few earlier numbers have a tag
and no release, from when tags were pushed by hand; they stay as they are.

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
