# Updater Contract (Sempal)

This document describes the release artifact shapes produced by `rls_cargo` and consumed by the in-app update checker / `sempal-updater` helper.

Canonical source of truth for naming and layout is `release_contract.toml`.

## Channels

- `stable`: immutable releases tagged `v{VERSION}` (GitHub `prerelease=false`)
- `nightly`: rolling release tagged `nightly` (GitHub `prerelease=true`)

## Asset naming

For `windows` + `x86_64` (`x86_64-pc-windows-msvc`):

- Stable zip: `sempal-v{VERSION}-windows-x86_64.zip`
- Nightly zip: `sempal-nightly-windows-x86_64.zip`
- Stable checksums: `checksums-v{VERSION}.txt`
- Nightly checksums: `checksums-nightly.txt`
- Stable checksums signature: `checksums-v{VERSION}.txt.sig`
- Nightly checksums signature: `checksums-nightly.txt.sig`

For `linux` + `x86_64` (`x86_64-unknown-linux-gnu`):

- Stable zip: `sempal-v{VERSION}-linux-x86_64.zip`
- Nightly zip: `sempal-nightly-linux-x86_64.zip`
- Stable checksums: `checksums-v{VERSION}.txt`
- Nightly checksums: `checksums-nightly.txt`
- Stable checksums signature: `checksums-v{VERSION}.txt.sig`
- Nightly checksums signature: `checksums-nightly.txt.sig`

For `linux` + `aarch64` (`aarch64-unknown-linux-gnu`):

- Stable zip: `sempal-v{VERSION}-linux-aarch64.zip`
- Nightly zip: `sempal-nightly-linux-aarch64.zip`
- Stable checksums: `checksums-v{VERSION}.txt`
- Nightly checksums: `checksums-nightly.txt`
- Stable checksums signature: `checksums-v{VERSION}.txt.sig`
- Nightly checksums signature: `checksums-nightly.txt.sig`

For `macos` + `x86_64` (`x86_64-apple-darwin`):

- Stable zip: `sempal-v{VERSION}-macos-x86_64.zip`
- Nightly zip: `sempal-nightly-macos-x86_64.zip`
- Stable checksums: `checksums-v{VERSION}.txt`
- Nightly checksums: `checksums-nightly.txt`
- Stable checksums signature: `checksums-v{VERSION}.txt.sig`
- Nightly checksums signature: `checksums-nightly.txt.sig`

For `macos` + `aarch64` (`aarch64-apple-darwin`):

- Stable zip: `sempal-v{VERSION}-macos-aarch64.zip`
- Nightly zip: `sempal-nightly-macos-aarch64.zip`
- Stable checksums: `checksums-v{VERSION}.txt`
- Nightly checksums: `checksums-nightly.txt`
- Stable checksums signature: `checksums-v{VERSION}.txt.sig`
- Nightly checksums signature: `checksums-nightly.txt.sig`

The checksums file is expected to include a SHA-256 entry for the zip asset:

```
<sha256>  <zip_filename>
```

The checksums signature file contains a base64-encoded Ed25519 signature of the
checksums file bytes.

CI signs the checksums file using OpenSSL and uploads the signature as
`checksums-...txt.sig`. The signature must be generated with the private key
stored in the `SEMPAL_CHECKSUMS_ED25519_KEY` GitHub Actions secret, and the app
embeds the corresponding public key for verification.

## Zip layout

The zip expands to exactly one root folder:

```
sempal/
  sempal(.exe)
  sempal-updater.exe       (windows only)
  models/panns_cnn14_16k.bpk
  update-manifest.json
  resources/            (optional)
```

## `update-manifest.json` schema

The updater validates at least:

- `app`: must be `sempal`
- `channel`: `stable` or `nightly`
- `target`: Rust target triple, e.g. `x86_64-pc-windows-msvc`
- `platform`: `windows`
- `arch`: `x86_64`
- `files`: list of file names expected inside `sempal/`

During updates, the updater compares the installed `update-manifest.json` against
the new manifest and deletes any previously listed files that are no longer
present. If the new release omits a `resources/` directory, the existing one is
removed as well. This keeps stale binaries or assets from lingering across
releases.

Minimal example:

```json
{
  "app": "sempal",
  "channel": "stable",
  "target": "x86_64-pc-windows-msvc",
  "platform": "windows",
  "arch": "x86_64",
  "files": ["sempal-updater.exe", "sempal.exe", "models/panns_cnn14_16k.bpk", "update-manifest.json"]
}
```
