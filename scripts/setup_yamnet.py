#!/usr/bin/env python3
import argparse
import importlib
import os
import platform
import shutil
import site
import subprocess
import sys
import tarfile
import urllib.request
import zipfile
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


def find_tflite_runtime() -> Path | None:
    candidates = []
    for base in site.getsitepackages() + [site.getusersitepackages()]:
        if not base:
            continue
        base_path = Path(base)
        if not base_path.exists():
            continue
        candidates.extend(base_path.rglob("*tensorflowlite_c.*"))
        candidates.extend(base_path.rglob("libtensorflowlite_c.*"))
    if not candidates:
        return None
    candidates.sort(key=lambda p: len(str(p)))
    return candidates[0]


def runtime_filename() -> str:
    system = platform.system().lower()
    if system == "windows":
        return "tensorflowlite_c.dll"
    if system == "darwin":
        return "libtensorflowlite_c.dylib"
    return "libtensorflowlite_c.so"


def runtime_urls(version: str) -> list[str]:
    system = platform.system().lower()
    arch = "x86_64"
    if system == "windows":
        base = f"https://storage.googleapis.com/tensorflow/libtensorflowlite_c/windows/{arch}"
        names = [
            f"tensorflowlite_c-{version}.zip",
            f"tensorflowlite_c-{version[:4]}.zip",
        ]
    elif system == "darwin":
        base = f"https://storage.googleapis.com/tensorflow/libtensorflowlite_c/darwin/{arch}"
        names = [
            f"libtensorflowlite_c-{version}.tar.gz",
            f"libtensorflowlite_c-{version[:4]}.tar.gz",
        ]
    else:
        base = f"https://storage.googleapis.com/tensorflow/libtensorflowlite_c/linux/{arch}"
        names = [
            f"libtensorflowlite_c-{version}.tar.gz",
            f"libtensorflowlite_c-{version[:4]}.tar.gz",
        ]
    return [f"{base}/{name}" for name in names]


def download_runtime(version: str, dest_dir: Path, override_url: str | None) -> Path | None:
    urls = [override_url] if override_url else runtime_urls(version)
    for url in urls:
        if not url:
            continue
        try:
            with urllib.request.urlopen(url, timeout=60) as resp:
                data = resp.read()
        except Exception:
            continue
        tmp = dest_dir / "tflite_runtime.tmp"
        tmp.write_bytes(data)
        extracted = extract_runtime(tmp, dest_dir)
        tmp.unlink(missing_ok=True)
        if extracted:
            return extracted
    return None


def extract_runtime(archive_path: Path, dest_dir: Path) -> Path | None:
    name = runtime_filename()
    if zipfile.is_zipfile(archive_path):
        with zipfile.ZipFile(archive_path, "r") as zf:
            for member in zf.namelist():
                if member.endswith(name):
                    zf.extract(member, dest_dir)
                    extracted = dest_dir / member
                    target = dest_dir / name
                    shutil.move(str(extracted), str(target))
                    return target
        return None
    try:
        with tarfile.open(archive_path, "r:*") as tf:
            for member in tf.getmembers():
                if member.name.endswith(name):
                    tf.extract(member, dest_dir)
                    extracted = dest_dir / member.name
                    target = dest_dir / name
                    shutil.move(str(extracted), str(target))
                    return target
    except tarfile.TarError:
        return None
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate yamnet.tflite for sempal.")
    parser.add_argument("--app-root", type=Path, help="Override app root directory")
    parser.add_argument("--no-install", action="store_true", help="Skip pip installs")
    parser.add_argument("--force", action="store_true", help="Overwrite existing model")
    parser.add_argument("--runtime-url", help="Override TFLite runtime download URL")
    args = parser.parse_args()

    app_root = args.app_root or resolve_app_root()
    models_dir = app_root / "models"
    models_dir.mkdir(parents=True, exist_ok=True)
    target = models_dir / "yamnet.tflite"
    runtime_dir = models_dir / "tflite"
    runtime_dir.mkdir(parents=True, exist_ok=True)

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

    import tensorflow as tf

    runtime = find_tflite_runtime()
    if runtime is None:
        version = getattr(tf, "__version__", "2.20.0")
        runtime = download_runtime(version, runtime_dir, args.runtime_url)
        if runtime is None:
            print("WARNING: Could not locate or download tensorflowlite_c runtime.")
            print("Please copy tensorflowlite_c.* into:", runtime_dir)
            return 0
    runtime_target = runtime_dir / runtime_filename()
    shutil.copy2(runtime, runtime_target)
    print(f"Copied TFLite runtime to {runtime_target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
