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


def log_mel_from_samples(samples, sr, n_fft, hop_length, n_mels, fmin, fmax):
    import numpy as np
    import librosa

    mel = librosa.feature.melspectrogram(
        y=samples,
        sr=sr,
        n_fft=n_fft,
        hop_length=hop_length,
        n_mels=n_mels,
        fmin=fmin,
        fmax=fmax,
        power=2.0,
    )
    return np.log10(np.maximum(mel, 1e-6))


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate golden PANNs embeddings via ONNX Runtime."
    )
    parser.add_argument(
        "--onnx",
        type=Path,
        default=Path("assets/ml/panns_cnn14/panns_cnn14.onnx"),
        help="Path to PANNs ONNX model",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("assets/ml/panns_cnn14/golden_embedding.json"),
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
    parser.add_argument("--sample-rate", type=int, default=32000, help="Sample rate")
    parser.add_argument("--n-fft", type=int, default=1024, help="FFT size")
    parser.add_argument("--hop-length", type=int, default=320, help="Hop length")
    parser.add_argument("--n-mels", type=int, default=64, help="Mel bands")
    parser.add_argument("--fmin", type=float, default=50.0, help="Mel fmin")
    parser.add_argument("--fmax", type=float, default=14000.0, help="Mel fmax")
    args = parser.parse_args()

    if not args.onnx.exists():
        raise RuntimeError(f"ONNX model not found: {args.onnx}")

    try:
        import numpy as np
        import librosa
    except Exception as err:
        raise RuntimeError(
            "numpy and librosa are required (pip install numpy librosa)"
        ) from err
    try:
        import onnxruntime as ort
    except Exception as err:
        raise RuntimeError("onnxruntime is required (pip install onnxruntime)") from err

    sr = int(args.sample_rate)
    tone_len = int(sr * args.tone_seconds)
    t = np.arange(tone_len, dtype=np.float32) / float(sr)
    tone = np.sin(2.0 * math.pi * args.tone_hz * t) * float(args.tone_amp)

    target_samples = int(sr * args.target_seconds)
    padded = repeat_pad(tone.tolist(), target_samples)
    padded = np.asarray(padded, dtype=np.float32)

    mel = log_mel_from_samples(
        padded,
        sr,
        int(args.n_fft),
        int(args.hop_length),
        int(args.n_mels),
        float(args.fmin),
        float(args.fmax),
    )
    frames = mel.shape[1]
    target_frames = int(round(float(sr) * float(args.target_seconds) / float(args.hop_length)))
    if frames < target_frames:
        pad_width = target_frames - frames
        mel = np.pad(mel, ((0, 0), (0, pad_width)), mode="constant")
    elif frames > target_frames:
        mel = mel[:, :target_frames]

    input_tensor = mel.astype("float32").reshape(1, 1, int(args.n_mels), target_frames)

    sess = ort.InferenceSession(str(args.onnx), providers=["CPUExecutionProvider"])
    input_name = sess.get_inputs()[0].name
    ort_out = sess.run(None, {input_name: input_tensor})[0]
    embedding = ort_out.reshape(-1).astype("float32")

    payload = {
        "model": "panns_cnn14",
        "sample_rate": sr,
        "tone_hz": float(args.tone_hz),
        "tone_amp": float(args.tone_amp),
        "tone_seconds": float(args.tone_seconds),
        "target_seconds": float(args.target_seconds),
        "embedding": embedding.tolist(),
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"Wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
