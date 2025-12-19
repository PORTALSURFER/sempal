#!/usr/bin/env python3
import argparse
import importlib
import os
import platform
import shutil
import subprocess
import sys
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


def try_import_tf() -> bool:
    try:
        importlib.import_module("tensorflow")
        importlib.import_module("tensorflow_hub")
        return True
    except Exception:
        return False


def ensure_tf(no_install: bool) -> None:
    if try_import_tf():
        return
    if no_install:
        raise RuntimeError(
            "tensorflow and tensorflow_hub are required. Install them first or omit --no-install."
        )
    print("Installing tensorflow + tensorflow_hub (this may take a while)...")
    subprocess.check_call([sys.executable, "-m", "pip", "install", "--upgrade", "pip"])

    def install(use_user: bool) -> bool:
        cmd = [sys.executable, "-m", "pip", "install"]
        if use_user:
            cmd.append("--user")
        cmd.extend(["tensorflow", "tensorflow_hub"])
        subprocess.check_call(cmd)
        if use_user:
            import site

            site.addsitedir(site.getusersitepackages())
        return try_import_tf()

    if install(True):
        return
    if install(False):
        return

    raise RuntimeError(
        "TensorFlow install completed but import still failed. "
        f"Try: {sys.executable} -m pip install tensorflow tensorflow_hub"
    )


def build_tflite() -> bytes:
    import tensorflow as tf
    import tensorflow_hub as hub

    model = hub.load("https://tfhub.dev/google/yamnet/1")
    concrete = model.signatures["serving_default"]
    converter = tf.lite.TFLiteConverter.from_concrete_functions([concrete])
    converter.optimizations = [tf.lite.Optimize.DEFAULT]
    return converter.convert()


def verify_tflite(path: Path) -> None:
    data = path.read_bytes()
    if len(data) < 8 or data[4:8] != b"TFL3":
        raise RuntimeError(f"{path} is not a valid .tflite file (missing TFL3 header)")
    if len(data) < 1024 * 100:
        raise RuntimeError(f"{path} is unexpectedly small ({len(data)} bytes)")


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate yamnet.tflite for sempal.")
    parser.add_argument("--app-root", type=Path, help="Override app root directory")
    parser.add_argument("--no-install", action="store_true", help="Skip pip installs")
    parser.add_argument("--force", action="store_true", help="Overwrite existing model")
    args = parser.parse_args()

    app_root = args.app_root or resolve_app_root()
    models_dir = app_root / "models"
    models_dir.mkdir(parents=True, exist_ok=True)
    target = models_dir / "yamnet.tflite"

    if target.exists() and not args.force:
        print(f"Model already exists at {target}. Use --force to overwrite.")
        return 0

    ensure_tf(args.no_install)
    tflite_data = build_tflite()
    tmp_path = models_dir / "yamnet.tflite.tmp"
    tmp_path.write_bytes(tflite_data)
    verify_tflite(tmp_path)
    shutil.move(str(tmp_path), str(target))
    print(f"Wrote {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
