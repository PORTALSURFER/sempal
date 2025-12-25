#!/usr/bin/env python3
import argparse
import math
from pathlib import Path


def load_audio(path: Path, target_sr: int):
    try:
        import soundfile as sf
    except Exception as err:
        raise RuntimeError("soundfile is required (pip install soundfile)") from err
    try:
        import numpy as np
    except Exception as err:
        raise RuntimeError("numpy is required (pip install numpy)") from err

    data, sr = sf.read(str(path), always_2d=True)
    if data.size == 0:
        raise RuntimeError(f"{path} decoded to empty audio")
    mono = data.mean(axis=1).astype("float32")
    if sr == target_sr:
        return mono, sr

    try:
        import librosa
        resampled = librosa.resample(mono, orig_sr=sr, target_sr=target_sr)
        return resampled.astype("float32"), target_sr
    except Exception:
        try:
            import torch
            import torchaudio
        except Exception as err:
            raise RuntimeError(
                "Resampling requires librosa or torchaudio (pip install librosa)"
            ) from err
        waveform = torch.tensor(mono).unsqueeze(0)
        resampler = torchaudio.transforms.Resample(sr, target_sr)
        resampled = resampler(waveform).squeeze(0).numpy()
        return resampled.astype("float32"), target_sr


def repeat_pad(samples, target_len):
    if target_len <= 0:
        return samples
    if samples.size == 0:
        return samples
    if samples.size >= target_len:
        return samples[:target_len]
    import numpy as np

    out = np.empty((target_len,), dtype=samples.dtype)
    filled = 0
    while filled < target_len:
        take = min(samples.size, target_len - filled)
        out[filled : filled + take] = samples[:take]
        filled += take
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
        description="Validate PANNs ONNX embedding output shape and norm."
    )
    parser.add_argument("audio_path", type=Path, help="Path to an audio file")
    parser.add_argument(
        "--onnx",
        type=Path,
        default=Path("assets/ml/panns_cnn14/panns_cnn14.onnx"),
        help="Path to PANNs ONNX model",
    )
    parser.add_argument("--sample-rate", type=int, default=32000, help="Sample rate")
    parser.add_argument("--seconds", type=float, default=10.0, help="Target seconds")
    parser.add_argument("--n-fft", type=int, default=1024, help="FFT size")
    parser.add_argument("--hop-length", type=int, default=320, help="Hop length")
    parser.add_argument("--n-mels", type=int, default=64, help="Mel bands")
    parser.add_argument("--fmin", type=float, default=50.0, help="Mel fmin")
    parser.add_argument("--fmax", type=float, default=14000.0, help="Mel fmax")
    parser.add_argument("--expected-dim", type=int, default=2048, help="Expected dim")
    args = parser.parse_args()

    if not args.audio_path.exists():
        raise RuntimeError(f"Audio file not found: {args.audio_path}")
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
    samples, _sr = load_audio(args.audio_path, sr)
    target_len = int(sr * args.seconds)
    samples = repeat_pad(samples, target_len)

    mel = log_mel_from_samples(
        samples,
        sr,
        int(args.n_fft),
        int(args.hop_length),
        int(args.n_mels),
        float(args.fmin),
        float(args.fmax),
    )
    target_frames = int(round(float(sr) * float(args.seconds) / float(args.hop_length)))
    frames = mel.shape[1]
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

    dim = int(embedding.shape[0])
    norm = math.sqrt(float((embedding * embedding).sum()))
    ok = dim == int(args.expected_dim)

    print(f"dim: {dim}")
    print(f"l2_norm: {norm:.6f}")
    print(f"status: {'OK' if ok else 'MISMATCH'} (expected_dim={args.expected_dim})")
    return 0 if ok else 2


if __name__ == "__main__":
    raise SystemExit(main())
