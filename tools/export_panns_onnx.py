#!/usr/bin/env python3
import argparse
import json
import os
import shutil
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

DEFAULT_MODEL_TYPE = "Cnn14_16k"
DEFAULT_CHECKPOINT_NAME = "Cnn14_16k_mAP=0.438.pth"
DEFAULT_CHECKPOINT_URL = (
    "https://zenodo.org/api/records/3987831/files/"
    "Cnn14_16k_mAP=0.438.pth/content"
)
DEFAULT_ONNX_NAME = "panns_cnn14_16k.onnx"


def download(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(url) as response, destination.open("wb") as handle:
        shutil.copyfileobj(response, handle)


def ensure_repo(tmp_dir: Path) -> Path:
    archive_url = (
        "https://github.com/qiuqiangkong/audioset_tagging_cnn/"
        "archive/refs/heads/master.zip"
    )
    archive_path = tmp_dir / "audioset_tagging_cnn.zip"
    download(archive_url, archive_path)
    with zipfile.ZipFile(archive_path) as archive:
        archive.extractall(tmp_dir)
    repo_root = tmp_dir / "audioset_tagging_cnn-master"
    pytorch_dir = repo_root / "pytorch"
    if not pytorch_dir.exists():
        raise RuntimeError("audioset_tagging_cnn pytorch directory not found")
    return pytorch_dir


def load_checkpoint(path: Path):
    import torch

    checkpoint = torch.load(path, map_location="cpu")
    if isinstance(checkpoint, dict) and "model" in checkpoint:
        return checkpoint["model"]
    return checkpoint


def build_logmel_export_model(base):
    import torch
    import torch.nn.functional as F

    class LogmelExportModel(torch.nn.Module):
        def __init__(self, base_model):
            super().__init__()
            self.base = base_model
            self.bn0 = base_model.bn0
            self.conv_block1 = base_model.conv_block1
            self.conv_block2 = base_model.conv_block2
            self.conv_block3 = base_model.conv_block3
            self.conv_block4 = base_model.conv_block4
            self.conv_block5 = base_model.conv_block5
            self.conv_block6 = base_model.conv_block6
            self.fc1 = base_model.fc1

        def forward(self, logmel):
            x = logmel
            x = x.transpose(1, 3)
            x = self.bn0(x)
            x = x.transpose(1, 3)

            x = self.conv_block1(x, pool_size=(2, 2), pool_type="avg")
            x = F.dropout(x, p=0.2, training=self.base.training)
            x = self.conv_block2(x, pool_size=(2, 2), pool_type="avg")
            x = F.dropout(x, p=0.2, training=self.base.training)
            x = self.conv_block3(x, pool_size=(2, 2), pool_type="avg")
            x = F.dropout(x, p=0.2, training=self.base.training)
            x = self.conv_block4(x, pool_size=(2, 2), pool_type="avg")
            x = F.dropout(x, p=0.2, training=self.base.training)
            x = self.conv_block5(x, pool_size=(2, 2), pool_type="avg")
            x = F.dropout(x, p=0.2, training=self.base.training)
            x = self.conv_block6(x, pool_size=(1, 1), pool_type="avg")
            x = F.dropout(x, p=0.2, training=self.base.training)
            x = x.mean(dim=3)

            x1, _ = x.max(dim=2)
            x2 = x.mean(dim=2)
            x = x1 + x2
            x = F.dropout(x, p=0.5, training=self.base.training)
            x = F.relu_(self.fc1(x))
            embedding = F.dropout(x, p=0.5, training=self.base.training)
            return embedding

    return LogmelExportModel(base)


def export_from_checkpoint(
    checkpoint: Path,
    output: Path,
    model_type: str,
    sample_rate: int,
    window_size: int,
    hop_size: int,
    mel_bins: int,
    fmin: int,
    fmax: int,
    seconds: float,
    opset: int,
) -> None:
    import torch

    with tempfile.TemporaryDirectory() as tmp:
        pytorch_dir = ensure_repo(Path(tmp))
        sys.path.insert(0, str(pytorch_dir))
        try:
            import models
        except Exception as err:
            raise RuntimeError(
                "Failed to import audioset_tagging_cnn models; ensure torchlibrosa is installed"
            ) from err

        model_cls = getattr(models, model_type)
        model = model_cls(
            sample_rate=sample_rate,
            window_size=window_size,
            hop_size=hop_size,
            mel_bins=mel_bins,
            fmin=fmin,
            fmax=fmax,
            classes_num=527,
        )
        state = load_checkpoint(checkpoint)
        model.load_state_dict(state)
        model.eval()

        wrapper = build_logmel_export_model(model)
        wrapper.eval()

        frames = int(round(sample_rate * seconds / hop_size))
        dummy = torch.randn(1, 1, frames, mel_bins)

        output.parent.mkdir(parents=True, exist_ok=True)
        torch.onnx.export(
            wrapper,
            dummy,
            output,
            input_names=["logmel"],
            output_names=["embedding"],
            dynamic_axes={"logmel": {0: "batch"}, "embedding": {0: "batch"}},
            opset_version=opset,
        )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Export a PANNs checkpoint to ONNX for SemPal."
    )
    parser.add_argument(
        "--checkpoint",
        type=Path,
        help="Path to PANNs checkpoint (.pth).",
    )
    parser.add_argument(
        "--checkpoint-url",
        default=os.environ.get("SEMPAL_PANNS_CHECKPOINT_URL", DEFAULT_CHECKPOINT_URL),
        help="URL to download the checkpoint if --checkpoint is missing.",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path("assets/ml/panns_cnn14_16k"),
        help="Output directory for ONNX + metadata",
    )
    parser.add_argument(
        "--onnx-name",
        default=DEFAULT_ONNX_NAME,
        help="ONNX filename to write inside out-dir",
    )
    parser.add_argument(
        "--model-type",
        default=DEFAULT_MODEL_TYPE,
        help="Model class name from audioset_tagging_cnn",
    )
    parser.add_argument("--sample-rate", type=int, default=16000)
    parser.add_argument("--window-size", type=int, default=512)
    parser.add_argument("--hop-size", type=int, default=160)
    parser.add_argument("--mel-bins", type=int, default=64)
    parser.add_argument("--fmin", type=int, default=50)
    parser.add_argument("--fmax", type=int, default=8000)
    parser.add_argument("--seconds", type=float, default=10.0)
    parser.add_argument("--opset", type=int, default=17)
    parser.add_argument(
        "--model-id",
        default="panns_cnn14_16k",
        help="Model identifier to store in metadata",
    )
    parser.add_argument(
        "--embedding-dim",
        type=int,
        default=2048,
        help="Override embedding dimension",
    )
    args = parser.parse_args()

    checkpoint = args.checkpoint
    if checkpoint is None:
        checkpoint = args.out_dir / DEFAULT_CHECKPOINT_NAME
        if not checkpoint.exists():
            if not args.checkpoint_url:
                raise RuntimeError("Checkpoint missing and no URL provided")
            print(f"Downloading checkpoint from {args.checkpoint_url}...")
            download(args.checkpoint_url, checkpoint)

    if not checkpoint.exists():
        raise RuntimeError(f"Checkpoint not found: {checkpoint}")

    args.out_dir.mkdir(parents=True, exist_ok=True)
    onnx_out = args.out_dir / args.onnx_name
    export_from_checkpoint(
        checkpoint,
        onnx_out,
        args.model_type,
        args.sample_rate,
        args.window_size,
        args.hop_size,
        args.mel_bins,
        args.fmin,
        args.fmax,
        args.seconds,
        args.opset,
    )

    meta = {
        "model": args.model_id,
        "input_names": ["logmel"],
        "input_shapes": [[1, 1, int(round(args.sample_rate * args.seconds / args.hop_size)), args.mel_bins]],
        "embedding_dim": args.embedding_dim,
    }
    meta_path = args.out_dir / "model_meta.json"
    meta_path.write_text(json.dumps(meta, indent=2) + "\n")

    print(f"Wrote {onnx_out}")
    print(f"Wrote {meta_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
