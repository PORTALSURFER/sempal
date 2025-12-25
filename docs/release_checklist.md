# Release Checklist (Windows)

## Build
- Run `scripts/build_installer.ps1` with access to the latest `panns_cnn14.onnx` and `onnxruntime.dll`.
- Confirm `dist/windows/bundle` contains `sempal.exe`, `sempal.ico`, and `models/` payloads.
- Verify `dist/windows/sempal-installer.exe` launches and shows the SemPal installer UI.

## Install Verification
- Install into a clean directory (e.g. `C:\Program Files\SemPal`).
- Confirm `%APPDATA%\.sempal\models\panns_cnn14.onnx` exists after install.
- Confirm `%APPDATA%\.sempal\models\onnxruntime\onnxruntime.dll` exists after install.
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
