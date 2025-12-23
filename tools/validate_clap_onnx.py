#!/usr/bin/env python3
import argparse
import json
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


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate CLAP audio encoder ONNX against HF transformers reference."
    )
    parser.add_argument("audio_path", type=Path, help="Path to an audio file")
    parser.add_argument(
        "--onnx",
        type=Path,
        default=Path("assets/ml/clap_v1/audio_encoder.onnx"),
        help="Path to audio_encoder.onnx",
    )
    parser.add_argument(
        "--model",
        default="laion/clap-htsat-fused",
        help="HF model id (default: laion/clap-htsat-fused)",
    )
    parser.add_argument(
        "--meta",
        type=Path,
        default=Path("assets/ml/clap_v1/model_meta.json"),
        help="Path to model_meta.json",
    )
    parser.add_argument("--atol", type=float, default=1e-3, help="Absolute tolerance")
    args = parser.parse_args()

    if not args.audio_path.exists():
        raise RuntimeError(f"Audio file not found: {args.audio_path}")
    if not args.onnx.exists():
        raise RuntimeError(f"ONNX model not found: {args.onnx}")

    try:
        import numpy as np
        import torch
        from transformers import ClapFeatureExtractor, ClapModel
    except Exception as err:
        raise RuntimeError(
            "transformers, torch, and numpy are required (pip install transformers torch numpy)"
        ) from err
    try:
        import onnxruntime as ort
    except Exception as err:
        raise RuntimeError("onnxruntime is required (pip install onnxruntime)") from err

    meta = json.loads(args.meta.read_text()) if args.meta.exists() else {}
    expected_samples = None
    if "input_shapes" in meta and meta["input_shapes"]:
        shape = meta["input_shapes"][0]
        if len(shape) >= 3:
            expected_samples = int(shape[-1])

    feature_extractor = ClapFeatureExtractor.from_pretrained(args.model)
    model = ClapModel.from_pretrained(args.model)
    model.eval()

    samples, sr = load_audio(args.audio_path, feature_extractor.sampling_rate)
    if expected_samples:
        samples = repeat_pad(samples, expected_samples)
    inputs = feature_extractor(samples, sampling_rate=sr, return_tensors="pt")
    with torch.no_grad():
        ref = model.get_audio_features(**inputs).cpu().numpy().astype("float32")
    ref = ref[0]

    sess = ort.InferenceSession(str(args.onnx), providers=["CPUExecutionProvider"])
    input_name = sess.get_inputs()[0].name
    input_shape = sess.get_inputs()[0].shape
    if len(input_shape) != 3:
        raise RuntimeError(
            f"ONNX input shape {input_shape} is not [B,C,T]; update this script accordingly."
        )
    batch = 1
    channels = int(input_shape[1]) if isinstance(input_shape[1], int) else 1
    target_len = expected_samples or samples.shape[0]
    samples = repeat_pad(samples, target_len)
    audio = samples.reshape(batch, channels, target_len).astype("float32")
    ort_out = sess.run(None, {input_name: audio})[0]
    ort_out = ort_out[0].astype("float32")

    diff = np.abs(ref - ort_out)
    max_diff = float(diff.max())
    mean_diff = float(diff.mean())
    l2_ref = math.sqrt(float((ref * ref).sum()))
    l2_ort = math.sqrt(float((ort_out * ort_out).sum()))
    ok = max_diff <= args.atol

    print(f"max_abs_diff: {max_diff:.6f}")
    print(f"mean_abs_diff: {mean_diff:.6f}")
    print(f"ref_l2: {l2_ref:.6f}")
    print(f"onnx_l2: {l2_ort:.6f}")
    print(f"status: {'OK' if ok else 'MISMATCH'} (atol={args.atol})")
    return 0 if ok else 2


if __name__ == "__main__":
    raise SystemExit(main())
