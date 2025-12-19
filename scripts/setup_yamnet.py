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
        importlib.import_module("tf2onnx")
        return True
    except Exception:
        return False


def ensure_tf(no_install: bool) -> None:
    if try_import_tf():
        return
    if no_install:
        raise RuntimeError(
            "tensorflow, tensorflow_hub, and tf2onnx are required. Install them first or omit --no-install."
        )
    print("Installing tensorflow + tensorflow_hub + tf2onnx (this may take a while)...")
    subprocess.check_call([sys.executable, "-m", "pip", "install", "--upgrade", "pip"])

    def install(use_user: bool) -> bool:
        cmd = [sys.executable, "-m", "pip", "install"]
        if use_user:
            cmd.append("--user")
        cmd.extend(["tensorflow", "tensorflow_hub", "tf2onnx", "onnx"])
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


def build_onnx() -> bytes:
    import tensorflow as tf
    import tensorflow_hub as hub
    import tf2onnx

    model = hub.load("https://tfhub.dev/google/yamnet/1")
    concrete = model.signatures["serving_default"]
    input_spec = (tf.TensorSpec([1, 15600], tf.float32, name="input"),)
    onnx_model, _ = tf2onnx.convert.from_function(
        concrete, input_signature=input_spec, opset=13, output_path=None
    )
    return onnx_model.SerializeToString()


def verify_onnx(path: Path) -> None:
    data = path.read_bytes()
    if len(data) < 1024 * 100:
        raise RuntimeError(f"{path} is unexpectedly small ({len(data)} bytes)")


def find_ort_runtime() -> Path | None:
    candidates = []
    for base in site.getsitepackages() + [site.getusersitepackages()]:
        if not base:
            continue
        base_path = Path(base)
        if not base_path.exists():
            continue
        candidates.extend(base_path.rglob("onnxruntime*.dll"))
        candidates.extend(base_path.rglob("libonnxruntime*.so"))
        candidates.extend(base_path.rglob("libonnxruntime*.dylib"))
    if not candidates:
        return None
    candidates.sort(key=lambda p: len(str(p)))
    return candidates[0]


def runtime_filename() -> str:
    system = platform.system().lower()
    if system == "windows":
        return "onnxruntime.dll"
    if system == "darwin":
        return "libonnxruntime.dylib"
    return "libonnxruntime.so"


def runtime_urls(version: str) -> list[str]:
    system = platform.system().lower()
    arch = "x64"
    base = f"https://github.com/microsoft/onnxruntime/releases/download/v{version}"
    if system == "windows":
        name = f"onnxruntime-win-{arch}-{version}.zip"
    elif system == "darwin":
        name = f"onnxruntime-osx-universal2-{version}.tgz"
    else:
        name = f"onnxruntime-linux-{arch}-{version}.tgz"
    return [f"{base}/{name}"]


def download_runtime(version: str, dest_dir: Path, override_url: str | None) -> Path | None:
    urls = [override_url] if override_url else runtime_urls(version)
    last_error = None
    for url in urls:
        if not url:
            continue
        try:
            with urllib.request.urlopen(url, timeout=60) as resp:
                data = resp.read()
        except Exception as err:
            last_error = err
            continue
        tmp = dest_dir / "onnxruntime.tmp"
        tmp.write_bytes(data)
        extracted = extract_runtime(tmp, dest_dir)
        tmp.unlink(missing_ok=True)
        if extracted:
            return extracted
    if last_error:
        print(f"Runtime download failed: {last_error}")
        print("Tried URLs:")
        for url in urls:
            if url:
                print(f"  - {url}")
    return None


def extract_runtime(archive_path: Path, dest_dir: Path) -> Path | None:
    name = runtime_filename()
    if archive_path.name.endswith((".dll", ".so", ".dylib")):
        target = dest_dir / name
        shutil.copy2(archive_path, target)
        return target
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
    parser = argparse.ArgumentParser(description="Generate yamnet.onnx for sempal.")
    parser.add_argument("--app-root", type=Path, help="Override app root directory")
    parser.add_argument("--no-install", action="store_true", help="Skip pip installs")
    parser.add_argument("--force", action="store_true", help="Overwrite existing model")
    parser.add_argument("--runtime-url", help="Override ONNX Runtime download URL")
    parser.add_argument("--runtime-file", type=Path, help="Use a local runtime archive/dll")
    parser.add_argument("--ort-version", default="1.18.1", help="ONNX Runtime version to download")
    args = parser.parse_args()

    app_root = args.app_root or resolve_app_root()
    models_dir = app_root / "models"
    models_dir.mkdir(parents=True, exist_ok=True)
    target = models_dir / "yamnet.onnx"
    runtime_dir = models_dir / "onnxruntime"
    runtime_dir.mkdir(parents=True, exist_ok=True)

    if target.exists() and not args.force:
        print(f"Model already exists at {target}. Use --force to overwrite.")
        return 0

    ensure_tf(args.no_install)
    onnx_data = build_onnx()
    tmp_path = models_dir / "yamnet.onnx.tmp"
    tmp_path.write_bytes(onnx_data)
    verify_onnx(tmp_path)
    shutil.move(str(tmp_path), str(target))
    print(f"Wrote {target}")

    runtime = None
    if args.runtime_file:
        runtime = extract_runtime(args.runtime_file, runtime_dir)
    if runtime is None:
        runtime = find_ort_runtime()
    if runtime is None:
        runtime = download_runtime(args.ort_version, runtime_dir, args.runtime_url)
        if runtime is None:
            print("WARNING: Could not locate or download ONNX Runtime.")
            print("Please copy onnxruntime.* into:", runtime_dir)
            return 0
    runtime_target = runtime_dir / runtime_filename()
    shutil.copy2(runtime, runtime_target)
    print(f"Copied ONNX Runtime to {runtime_target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
