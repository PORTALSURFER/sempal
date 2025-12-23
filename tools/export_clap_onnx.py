#!/usr/bin/env python3
import argparse
import json
from pathlib import Path


def export_clap_audio_encoder(
    target: Path, checkpoint: Path | None, channels: int, samples: int, opset: int
) -> int:
    import torch
    from laion_clap import CLAP_Module

    device = "cpu"
    model = CLAP_Module(enable_fusion=False)
    if checkpoint:
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
    with torch.no_grad():
        embedding = wrapper(dummy)
    embedding_dim = int(embedding.shape[-1])

    torch.onnx.export(
        wrapper,
        dummy,
        target,
        input_names=["audio"],
        output_names=["embedding"],
        dynamic_axes={
            "audio": {0: "batch"},
            "embedding": {0: "batch"},
        },
        opset_version=opset,
    )

    return embedding_dim


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Export CLAP audio encoder ONNX and metadata."
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path("assets/ml/clap_v1"),
        help="Output directory for audio_encoder.onnx",
    )
    parser.add_argument("--checkpoint", type=Path, help="Path to a CLAP checkpoint (.pt)")
    parser.add_argument("--sample-rate", type=int, default=48000, help="Input sample rate")
    parser.add_argument("--seconds", type=float, default=10.0, help="Input duration in seconds")
    parser.add_argument("--channels", type=int, default=1, help="Input channel count")
    parser.add_argument("--opset", type=int, default=17, help="ONNX opset version")
    args = parser.parse_args()

    args.out_dir.mkdir(parents=True, exist_ok=True)
    onnx_path = args.out_dir / "audio_encoder.onnx"
    meta_path = args.out_dir / "model_meta.json"
    input_samples = int(args.sample_rate * args.seconds)

    embedding_dim = export_clap_audio_encoder(
        onnx_path, args.checkpoint, args.channels, input_samples, args.opset
    )

    meta = {
        "model": "laion/clap-htsat-fused",
        "input_names": ["audio"],
        "input_shapes": [[1, args.channels, input_samples]],
        "opset": args.opset,
        "embedding_dim": embedding_dim,
    }
    meta_path.write_text(json.dumps(meta, indent=2) + "\n")

    print(f"Wrote {onnx_path}")
    print(f"Wrote {meta_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
