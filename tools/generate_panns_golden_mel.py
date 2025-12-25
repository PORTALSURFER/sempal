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
        description="Generate golden PANNs log-mel data for Rust tests."
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("assets/ml/panns_cnn14_16k/golden_mel.json"),
        help="Output JSON path",
    )
    parser.add_argument("--tone-hz", type=float, default=440.0, help="Tone frequency")
    parser.add_argument("--tone-amp", type=float, default=0.5, help="Tone amplitude")
    parser.add_argument(
        "--tone-seconds", type=float, default=0.5, help="Tone duration seconds"
    )
    parser.add_argument(
        "--target-seconds", type=float, default=10.0, help="Target padded duration"
    )
    parser.add_argument("--sample-rate", type=int, default=16000, help="Sample rate")
    parser.add_argument("--n-fft", type=int, default=512, help="FFT size")
    parser.add_argument("--hop-length", type=int, default=160, help="Hop length")
    parser.add_argument("--n-mels", type=int, default=64, help="Mel bands")
    parser.add_argument("--fmin", type=float, default=50.0, help="Mel fmin")
    parser.add_argument("--fmax", type=float, default=8000.0, help="Mel fmax")
    args = parser.parse_args()

    try:
        import numpy as np
        import librosa
    except Exception as err:
        raise RuntimeError(
            "numpy and librosa are required (pip install numpy librosa)"
        ) from err

    sr = int(args.sample_rate)
    tone_len = int(sr * args.tone_seconds)
    t = np.arange(tone_len, dtype=np.float32) / float(sr)
    tone = np.sin(2.0 * math.pi * args.tone_hz * t) * float(args.tone_amp)

    target_samples = int(sr * args.target_seconds)
    padded = repeat_pad(tone.tolist(), target_samples)
    padded = np.asarray(padded, dtype=np.float32)

    mel = librosa.feature.melspectrogram(
        y=padded,
        sr=sr,
        n_fft=int(args.n_fft),
        hop_length=int(args.hop_length),
        n_mels=int(args.n_mels),
        fmin=float(args.fmin),
        fmax=float(args.fmax),
        power=2.0,
    )
    log_mel = 10.0 * np.log10(np.maximum(mel, 1e-10))
    frames = log_mel.T.tolist()

    payload = {
        "sample_rate": sr,
        "n_fft": int(args.n_fft),
        "hop_length": int(args.hop_length),
        "n_mels": int(args.n_mels),
        "fmin": float(args.fmin),
        "fmax": float(args.fmax),
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
