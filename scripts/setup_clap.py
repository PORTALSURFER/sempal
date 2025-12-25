#!/usr/bin/env python3
import argparse
import importlib
import os
import platform
import shutil
import time
import site
import subprocess
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path


DEFAULT_CHECKPOINT_URLS = [
    "https://huggingface.co/lukewys/laion_clap/resolve/main/630k-audioset-fusion-best.pt",
    "https://huggingface.co/lukewys/laion_clap/resolve/main/630k-audioset-fusion.pt",
    "https://huggingface.co/lukewys/laion_clap/resolve/main/630k-audioset-fused.pt",
]
DEFAULT_CHECKPOINT_NAME = "clap_htsat_fused.pt"


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


def try_import_clap() -> tuple[bool, str | None]:
    try:
        importlib.import_module("torch")
        importlib.import_module("laion_clap")
        importlib.import_module("onnx")
        importlib.import_module("onnxscript")
        return True, None
    except Exception as err:
        return False, str(err)


def ensure_clap(no_install: bool) -> None:
    ok, err = try_import_clap()
    if ok:
        return
    if no_install:
        raise RuntimeError(
            "torch, laion_clap, and onnx are required. Install them first or omit --no-install."
        )
    print(
        "Installing torch + torchaudio + torchvision + laion-clap + onnx + onnxscript "
        "(this may take a while)..."
    )
    subprocess.check_call([sys.executable, "-m", "pip", "install", "--upgrade", "pip"])

    def install(use_user: bool) -> bool:
        cmd = [sys.executable, "-m", "pip", "install"]
        if use_user:
            cmd.append("--user")
        cmd.extend(["torch", "torchaudio", "torchvision", "laion-clap", "onnx", "onnxscript"])
        subprocess.check_call(cmd)
        if use_user:
            site.addsitedir(site.getusersitepackages())
        ok, _err = try_import_clap()
        return ok

    if install(True):
        return
    if install(False):
        return

    print("Retrying with --force-reinstall (this may take a while)...")
    subprocess.check_call(
        [
            sys.executable,
            "-m",
            "pip",
            "install",
            "--force-reinstall",
            "--no-cache-dir",
            "torch",
            "torchaudio",
            "torchvision",
            "laion-clap",
            "onnx",
            "onnxscript",
        ]
    )
    ok, err = try_import_clap()
    if ok:
        return

    raise RuntimeError(
        "CLAP install completed but import still failed. "
        f"Import error: {err}. "
        f"Try: {sys.executable} -m pip install torch torchaudio torchvision laion-clap onnx onnxscript"
    )


def build_onnx(target: Path, checkpoint: Path | None, channels: int, samples: int, opset: int) -> None:
    import torch
    from laion_clap import CLAP_Module

    device = "cpu"
    model = CLAP_Module(enable_fusion=False)
    if checkpoint:
        try:
            state = torch.load(checkpoint, map_location=device, weights_only=False)
        except TypeError:
            state = torch.load(checkpoint, map_location=device)
        if isinstance(state, dict) and "state_dict" in state:
            state = state["state_dict"]
        model.load_state_dict(state, strict=False)
    elif hasattr(model, "load_ckpt"):
        model.load_ckpt()
    else:
        raise RuntimeError("CLAP checkpoint required; pass --checkpoint.")
    model.eval().to(device)

    class ClapAudioWrapper(torch.nn.Module):
        def __init__(self, inner):
            super().__init__()
            self.inner = inner

        def forward(self, audio):
            if audio.dim() == 3 and audio.size(1) == 1:
                audio = audio[:, 0, :]
            return self.inner.get_audio_embedding_from_data(audio, use_tensor=True)

    wrapper = ClapAudioWrapper(model).to(device)
    dummy = torch.randn(1, channels, samples, device=device)
    export_kwargs = dict(
        input_names=["audio"],
        output_names=["embedding"],
        dynamic_axes={
            "audio": {0: "batch"},
            "embedding": {0: "batch"},
        },
        opset_version=opset,
    )
    try:
        torch.onnx.export(
            wrapper,
            dummy,
            target,
            **export_kwargs,
            dynamo=False,
        )
    except TypeError:
        torch.onnx.export(wrapper, dummy, target, **export_kwargs)


def download_checkpoint(urls: list[str], dest: Path) -> Path:
    if dest.exists():
        print(f"Using cached checkpoint at {dest}")
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    errors: list[str] = []
    for url in urls:
        if not url:
            continue
        tmp_path = dest.with_suffix(".tmp")
        print(f"Downloading CLAP checkpoint from {url}...")
        try:
            with urllib.request.urlopen(url, timeout=60) as resp, tmp_path.open("wb") as out:
                while True:
                    chunk = resp.read(1024 * 1024)
                    if not chunk:
                        break
                    out.write(chunk)
        except Exception as err:
            tmp_path.unlink(missing_ok=True)
            errors.append(f"{url}: {err}")
            continue
        tmp_path.replace(dest)
        print(f"Saved checkpoint to {dest}")
        return dest
    details = "\n".join(f"- {entry}" for entry in errors)
    raise RuntimeError(
        "Failed to download CLAP checkpoint. Tried:\n"
        f"{details}\n"
        "Pass --checkpoint or --checkpoint-url to override."
    )


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


def runtime_urls(version: str, flavor: str) -> list[str]:
    system = platform.system().lower()
    arch = "x64"
    base = f"https://github.com/microsoft/onnxruntime/releases/download/v{version}"
    if system == "windows":
        if flavor == "directml":
            name = f"onnxruntime-win-{arch}-directml-{version}.zip"
        elif flavor == "cuda":
            name = f"onnxruntime-win-{arch}-gpu-{version}.zip"
        else:
            name = f"onnxruntime-win-{arch}-{version}.zip"
    elif system == "darwin":
        name = f"onnxruntime-osx-universal2-{version}.tgz"
    else:
        name = f"onnxruntime-linux-{arch}-{version}.tgz"
    return [f"{base}/{name}"]


def download_runtime(
    version: str,
    flavor: str,
    dest_dir: Path,
    override_url: str | None,
) -> Path | None:
    urls = [override_url] if override_url else runtime_urls(version, flavor)
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
    parser = argparse.ArgumentParser(description="Generate clap_audio.onnx for sempal.")
    parser.add_argument("--app-root", type=Path, help="Override app root directory")
    parser.add_argument("--no-install", action="store_true", help="Skip pip installs")
    parser.add_argument("--force", action="store_true", help="Overwrite existing model")
    parser.add_argument("--runtime-url", help="Override ONNX Runtime download URL")
    parser.add_argument("--runtime-file", type=Path, help="Use a local runtime archive/dll")
    parser.add_argument("--ort-version", default="1.22.0", help="ONNX Runtime version to download")
    parser.add_argument(
        "--ort-flavor",
        default="cpu",
        choices=["cpu", "directml", "cuda"],
        help="ONNX Runtime flavor to download (Windows only)",
    )
    parser.add_argument("--checkpoint", type=Path, help="Path to a CLAP checkpoint (.pt)")
    parser.add_argument(
        "--checkpoint-url",
        default=None,
        help="Checkpoint URL to download when --checkpoint is not provided.",
    )
    parser.add_argument(
        "--no-checkpoint-download",
        action="store_true",
        help="Skip downloading a checkpoint and rely on laion-clap defaults.",
    )
    parser.add_argument("--sample-rate", type=int, default=48000, help="Input sample rate")
    parser.add_argument("--seconds", type=float, default=10.0, help="Input duration in seconds")
    parser.add_argument("--channels", type=int, default=1, help="Input channel count")
    parser.add_argument("--opset", type=int, default=17, help="ONNX opset version")
    args = parser.parse_args()

    app_root = args.app_root or resolve_app_root()
    models_dir = app_root / "models"
    models_dir.mkdir(parents=True, exist_ok=True)
    target = models_dir / "clap_audio.onnx"
    runtime_dir = models_dir / "onnxruntime"
    runtime_dir.mkdir(parents=True, exist_ok=True)

    if target.exists() and not args.force:
        print(f"Model already exists at {target}. Use --force to overwrite.")
        return 0

    ensure_clap(args.no_install)
    checkpoint = args.checkpoint
    if checkpoint is None and not args.no_checkpoint_download:
        urls = [args.checkpoint_url] if args.checkpoint_url else DEFAULT_CHECKPOINT_URLS
        checkpoint = download_checkpoint(urls, models_dir / DEFAULT_CHECKPOINT_NAME)
    input_samples = int(args.sample_rate * args.seconds)
    tmp_path = models_dir / "clap_audio.onnx.tmp"
    build_onnx(tmp_path, checkpoint, args.channels, input_samples, args.opset)
    verify_onnx(tmp_path)
    shutil.move(str(tmp_path), str(target))
    print(f"Wrote {target}")

    runtime = None
    if args.runtime_file:
        runtime = extract_runtime(args.runtime_file, runtime_dir)
    if runtime is None:
        runtime = find_ort_runtime()
    if runtime is None:
        runtime = download_runtime(
            args.ort_version,
            args.ort_flavor,
            runtime_dir,
            args.runtime_url,
        )
        if runtime is None:
            print("WARNING: Could not locate or download ONNX Runtime.")
            print("Please copy onnxruntime.* into:", runtime_dir)
            return 0
    runtime_target = runtime_dir / runtime_filename()
    if runtime_target.exists():
        print(f"ONNX Runtime already present at {runtime_target}")
        return 0
    copy_error = None
    for _ in range(5):
        try:
            shutil.copy2(runtime, runtime_target)
            copy_error = None
            break
        except PermissionError as err:
            copy_error = err
            time.sleep(0.5)
    if copy_error:
        print(f"WARNING: Failed to copy ONNX Runtime to {runtime_target}: {copy_error}")
        print("Close any running app using onnxruntime.dll and rerun the script.")
        return 0
    print(f"Copied ONNX Runtime to {runtime_target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
