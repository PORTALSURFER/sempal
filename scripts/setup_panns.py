#!/usr/bin/env python3
import argparse
import os
import platform
import shutil
import urllib.request
from pathlib import Path


def resolve_app_root() -> Path:
    override = os.environ.get("SEMPAL_CONFIG_HOME")
    if override:
        return Path(override) / ".sempal"
    system = platform.system().lower()
    if system == "windows":
        base = os.environ.get("APPDATA")
        if not base:
            raise RuntimeError("APPDATA is not set; set SEMPAL_CONFIG_HOME instead.")
        return Path(base) / ".sempal"
    if system == "darwin":
        return Path.home() / "Library" / "Application Support" / ".sempal"
    base = os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))
    return Path(base) / ".sempal"

def download_onnx(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(url) as response, destination.open("wb") as handle:
        shutil.copyfileobj(response, handle)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Install or update the PANNs ONNX model for SemPal."
    )
    parser.add_argument("--app-root", type=Path, help="Override app root directory")
    parser.add_argument(
        "--onnx",
        type=Path,
        default=Path("assets/ml/panns_cnn14/panns_cnn14.onnx"),
        help="Path to panns_cnn14.onnx",
    )
    parser.add_argument(
        "--onnx-url",
        help="Download URL for panns_cnn14.onnx when not found locally",
    )
    parser.add_argument(
        "--runtime-file",
        type=Path,
        help="Path to onnxruntime library to copy into models/onnxruntime",
    )
    parser.add_argument("--force", action="store_true", help="Overwrite existing files")
    args = parser.parse_args()

    app_root = args.app_root if args.app_root else resolve_app_root()
    models_dir = app_root / "models"
    models_dir.mkdir(parents=True, exist_ok=True)

    if not args.onnx.exists():
        onnx_url = args.onnx_url or os.environ.get("SEMPAL_PANNS_ONNX_URL")
        if not onnx_url:
            raise RuntimeError(
                f"ONNX model not found: {args.onnx}. "
                "Provide --onnx or set SEMPAL_PANNS_ONNX_URL to download it."
            )
        print(f"Downloading PANNs ONNX from {onnx_url}...")
        download_onnx(onnx_url, args.onnx)
        if not args.onnx.exists():
            raise RuntimeError(f"ONNX download failed: {args.onnx}")

    target_onnx = models_dir / "panns_cnn14.onnx"
    if target_onnx.exists() and not args.force:
        print(f"{target_onnx} already exists; pass --force to overwrite")
    else:
        shutil.copy2(args.onnx, target_onnx)
        print(f"Wrote {target_onnx}")

    if args.runtime_file:
        if not args.runtime_file.exists():
            raise RuntimeError(f"Runtime file not found: {args.runtime_file}")
        runtime_dir = models_dir / "onnxruntime"
        runtime_dir.mkdir(parents=True, exist_ok=True)
        target_runtime = runtime_dir / args.runtime_file.name
        if target_runtime.exists() and not args.force:
            print(f"{target_runtime} already exists; pass --force to overwrite")
        else:
            shutil.copy2(args.runtime_file, target_runtime)
            print(f"Wrote {target_runtime}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
