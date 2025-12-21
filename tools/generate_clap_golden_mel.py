#!/usr/bin/env python3
import argparse
import json
import math
from pathlib import Path


def repeat_pad(samples, target_len: int):
    if target_len <= 0:
        return []
    if len(samples) >= target_len:
        return samples[:target_len]
    out = []
    while len(out) < target_len:
        take = min(len(samples), target_len - len(out))
        out.extend(samples[:take])
    return out


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate golden CLAP log-mel data for Rust tests."
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("assets/ml/clap_v1/golden_mel.json"),
        help="Output JSON path",
    )
    parser.add_argument(
        "--model",
        default="laion/clap-htsat-fused",
        help="HF model id (default: laion/clap-htsat-fused)",
    )
    parser.add_argument("--tone-hz", type=float, default=440.0, help="Tone frequency")
    parser.add_argument("--tone-amp", type=float, default=0.5, help="Tone amplitude")
    parser.add_argument(
        "--tone-seconds", type=float, default=0.5, help="Tone duration seconds"
    )
    parser.add_argument(
        "--target-seconds", type=float, default=10.0, help="Target padded duration"
    )
    args = parser.parse_args()

    try:
        import numpy as np
        from transformers import ClapFeatureExtractor
    except Exception as err:
        raise RuntimeError(
            "numpy and transformers are required (pip install numpy transformers)"
        ) from err

    feature_extractor = ClapFeatureExtractor.from_pretrained(args.model)
    sr = int(feature_extractor.sampling_rate)
    tone_len = int(sr * args.tone_seconds)
    t = np.arange(tone_len, dtype=np.float32) / float(sr)
    tone = np.sin(2.0 * math.pi * args.tone_hz * t) * float(args.tone_amp)

    target_samples = int(sr * args.target_seconds)
    padded = repeat_pad(tone.tolist(), target_samples)
    padded = np.asarray(padded, dtype=np.float32)

    inputs = feature_extractor(
        padded,
        sampling_rate=sr,
        return_tensors="np",
    )
    features = inputs["input_features"][0]
    frames = features.T.tolist()

    payload = {
        "model": args.model,
        "sample_rate": sr,
        "n_fft": int(feature_extractor.n_fft),
        "hop_length": int(feature_extractor.hop_length),
        "n_mels": int(feature_extractor.n_mels),
        "fmin": float(feature_extractor.fmin),
        "fmax": float(feature_extractor.fmax),
        "tone_hz": float(args.tone_hz),
        "tone_amp": float(args.tone_amp),
        "tone_seconds": float(args.tone_seconds),
        "target_seconds": float(args.target_seconds),
        "mel_frames": frames,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"Wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
