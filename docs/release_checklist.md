# Release Checklist (Portable Bundles)

## Build
- Update local `main` before running release tooling (e.g. `git fetch origin main` and `git reset --hard origin/main`).
- Ensure `assets/ml/panns_cnn14_16k/panns_cnn14_16k.bpk` is present before builds.
- Run the release workflow or `scripts/build_release_zip.sh` per target.
- Confirm the zip includes the app binary, `models/panns_cnn14_16k.bpk`, and `update-manifest.json`.

## Portable Verification
- Extract the zip into a clean directory.
- Confirm `%APPDATA%\.sempal\models\panns_cnn14_16k.bpk` exists after install.
- Launch SemPal and run analysis on a sample pack; ensure embeddings are created.

## ML Release Criteria
- Gating precision/coverage curve matches expected ranges (no regression in margins).
- Golden tests pass (mel, embedding) with no drift.

## Cleanup Verification
- Delete the extracted directory and ensure no files remain there.
