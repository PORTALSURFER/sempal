# Release Checklist (Windows)

## Build
- Update local `main` before running release tooling (e.g. `git fetch origin main` and `git reset --hard origin/main`).
- Run `scripts/build_installer.ps1` (requires Python + PANNs export deps unless `SEMPAL_PANNS_ONNX_PATH` is already set).
- Confirm `dist/windows/bundle` contains `sempal.exe` and `sempal.ico`.
- Verify `dist/windows/sempal-installer.exe` launches and shows the SemPal installer UI.

## macOS/Linux installers
- Ensure Python + PANNs export deps are available for macOS/Linux builds unless `SEMPAL_PANNS_ONNX_PATH` is already set.
- Confirm release assets include `sempal-*-macos-*.pkg` and `sempal-*-linux-*.tar.gz`.

## Install Verification
- Install into a clean directory (e.g. `C:\Program Files\SemPal`).
- Confirm `%APPDATA%\.sempal\models\panns_cnn14_16k.bpk` exists after install.
- Launch SemPal and run analysis on a sample pack; ensure embeddings are created.

## ML Release Criteria
- Gating precision/coverage curve matches expected ranges (no regression in margins).
- Golden tests pass (mel, embedding) with no drift.

## Uninstall Verification
- Check SemPal appears in Add/Remove Programs.
- Run uninstall from Add/Remove Programs or `sempal-installer.exe --uninstall`.
- Confirm install directory is removed and uninstall entry disappears.

## Signing (Optional)
- Set `SIGNTOOL_PATH` and `SIGN_CERT_PATH` and rerun `scripts/build_installer.ps1 -Sign`.
- Verify signatures on `sempal-installer.exe` and `sempal.exe`.
