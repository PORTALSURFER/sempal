#!/usr/bin/env python3
import argparse
import json
import shutil
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Stage a PANNs ONNX model and write metadata."
    )
    parser.add_argument("--onnx", type=Path, required=True, help="Path to PANNs ONNX")
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path("assets/ml/panns_cnn14"),
        help="Output directory for ONNX + metadata",
    )
    parser.add_argument(
        "--model-id",
        default="panns_cnn14",
        help="Model identifier to store in metadata",
    )
    parser.add_argument(
        "--embedding-dim",
        type=int,
        default=None,
        help="Override embedding dimension",
    )
    args = parser.parse_args()

    if not args.onnx.exists():
        raise RuntimeError(f"ONNX model not found: {args.onnx}")

    args.out_dir.mkdir(parents=True, exist_ok=True)
    onnx_out = args.out_dir / "panns_cnn14.onnx"
    shutil.copy2(args.onnx, onnx_out)

    input_shapes = []
    embedding_dim = args.embedding_dim
    try:
        import onnxruntime as ort

        sess = ort.InferenceSession(str(onnx_out), providers=["CPUExecutionProvider"])
        input_shapes = [inp.shape for inp in sess.get_inputs()]
        output_shapes = [out.shape for out in sess.get_outputs()]
        if embedding_dim is None and output_shapes:
            last_dim = output_shapes[0][-1]
            if isinstance(last_dim, int):
                embedding_dim = int(last_dim)
    except Exception:
        pass

    meta = {
        "model": args.model_id,
        "input_names": ["audio"],
        "input_shapes": input_shapes,
        "embedding_dim": embedding_dim,
    }
    meta_path = args.out_dir / "model_meta.json"
    meta_path.write_text(json.dumps(meta, indent=2) + "\n")

    print(f"Wrote {onnx_out}")
    print(f"Wrote {meta_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
