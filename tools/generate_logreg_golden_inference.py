#!/usr/bin/env python3
import argparse
import json
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate a golden softmax LR inference sample."
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("assets/ml/clap_v1/golden_infer.json"),
        help="Output JSON path",
    )
    parser.add_argument("--dim", type=int, default=8)
    parser.add_argument("--classes", type=int, default=3)
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--temperature", type=float, default=1.3)
    args = parser.parse_args()

    try:
        import numpy as np
    except Exception as err:
        raise RuntimeError("numpy is required (pip install numpy)") from err

    rng = np.random.default_rng(args.seed)
    embedding = rng.normal(size=(args.dim,)).astype("float32")
    weights = rng.normal(size=(args.classes, args.dim)).astype("float32")
    bias = rng.normal(size=(args.classes,)).astype("float32")
    logits = (weights @ embedding + bias) / float(args.temperature)
    logits = logits - logits.max()
    exp = np.exp(logits)
    probs = (exp / exp.sum()).astype("float32")

    payload = {
        "dim": int(args.dim),
        "num_classes": int(args.classes),
        "temperature": float(args.temperature),
        "embedding": embedding.tolist(),
        "weights": weights.reshape(-1).tolist(),
        "bias": bias.tolist(),
        "probs": probs.tolist(),
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"Wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
