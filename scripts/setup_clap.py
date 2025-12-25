#!/usr/bin/env python3
import argparse
import importlib
import json
import os
import platform
import shutil
import time
import site
import subprocess
import sys
import tarfile
import urllib.error
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


def build_onnx(
    target: Path,
    checkpoint: Path | None,
    channels: int,
    samples: int,
    opset: int,
    static_shapes: bool,
    layernorm_dim: int | None,
) -> None:
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
        opset_version=opset,
    )
    if not static_shapes:
        export_kwargs["dynamic_axes"] = {
            "audio": {0: "batch"},
            "embedding": {0: "batch"},
        }
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

    if patch_layer_norm_weights(target, layernorm_dim):
        print("Patched LayerNormalization weights for Burn import compatibility.")


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


def _shape_map(model) -> dict[str, list[int | None]]:
    def dims_from_vi(vi):
        if not vi or not vi.type or not vi.type.tensor_type:
            return None
        shape = vi.type.tensor_type.shape
        if not shape:
            return None
        dims = []
        for dim in shape.dim:
            if dim.HasField("dim_value"):
                dims.append(int(dim.dim_value))
            else:
                dims.append(None)
        return dims

    mapping = {}
    for vi in list(model.graph.input) + list(model.graph.value_info) + list(model.graph.output):
        dims = dims_from_vi(vi)
        if dims is not None:
            mapping[vi.name] = dims
    return mapping


def patch_layer_norm_weights(path: Path, fallback_dim: int | None) -> bool:
    import onnx
    from onnx import numpy_helper, shape_inference
    import numpy as np

    model = onnx.load(path)
    try:
        model = shape_inference.infer_shapes(model)
    except Exception:
        pass

    shape_map = _shape_map(model)
    initializer_names = {init.name for init in model.graph.initializer}
    patched = False

    for node in model.graph.node:
        if node.op_type != "LayerNormalization":
            continue

        weight_name = node.input[1] if len(node.input) > 1 else ""
        if weight_name and weight_name in initializer_names:
            continue

        input_name = node.input[0] if node.input else ""
        dims = shape_map.get(input_name)
        if not dims or dims[-1] is None:
            output_name = node.output[0] if node.output else ""
            dims = shape_map.get(output_name)

        if not dims or dims[-1] is None:
            if fallback_dim is None:
                raise RuntimeError(
                    "Unable to infer LayerNorm weight shape. "
                    f"Missing or dynamic last-dim for input '{input_name}'."
                )
            dims = [fallback_dim]

        num_features = dims[-1]
        scale_name = weight_name or (f"{node.name}_scale" if node.name else "layernorm_scale")
        scale_init = numpy_helper.from_array(
            np.ones((num_features,), dtype=np.float32), name=scale_name
        )
        model.graph.initializer.append(scale_init)
        initializer_names.add(scale_name)

        if len(node.input) > 1:
            node.input[1] = scale_name
        else:
            node.input.append(scale_name)

        patched = True

    if patched:
        onnx.save(model, path)
    return patched


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
    assets = release_asset_urls(version, flavor, system, arch)
    if assets:
        return assets
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


def release_asset_urls(version: str, flavor: str, system: str, arch: str) -> list[str]:
    if system != "windows":
        return []
    url = f"https://api.github.com/repos/microsoft/onnxruntime/releases/tags/v{version}"
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except (urllib.error.URLError, json.JSONDecodeError):
        return []
    assets = payload.get("assets", [])
    if not isinstance(assets, list):
        return []
    matches: list[str] = []
    for asset in assets:
        name = str(asset.get("name", "")).lower()
        download = asset.get("browser_download_url")
        if not download:
            continue
        if f"win-{arch}" not in name:
            continue
        if flavor == "directml":
            if "directml" not in name:
                continue
        elif flavor == "cuda":
            if "gpu" not in name:
                continue
        else:
            if "directml" in name or "gpu" in name:
                continue
        matches.append(str(download))
    return matches


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
    parser.add_argument(
        "--static-shapes",
        action="store_true",
        help="Export ONNX with static batch size (helps Burn import).",
    )
    parser.add_argument(
        "--layernorm-dim",
        type=int,
        default=None,
        help="Fallback LayerNorm dim when shape inference fails (e.g., 768 for CLAP).",
    )
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
    env_fallback = os.environ.get("SEMPAL_CLAP_LAYERNORM_DIM")
    layernorm_dim = args.layernorm_dim or (int(env_fallback) if env_fallback else None)
    build_onnx(
        tmp_path,
        checkpoint,
        args.channels,
        input_samples,
        args.opset,
        args.static_shapes,
        layernorm_dim,
    )
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
