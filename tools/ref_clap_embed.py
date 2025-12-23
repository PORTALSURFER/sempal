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


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Reference CLAP embedding script (HF transformers)"
    )
    parser.add_argument("audio_path", type=Path, help="Path to an audio file")
    parser.add_argument(
        "--model",
        default="laion/clap-htsat-fused",
        help="HF model id (default: laion/clap-htsat-fused)",
    )
    args = parser.parse_args()

    if not args.audio_path.exists():
        raise RuntimeError(f"Audio file not found: {args.audio_path}")

    try:
        import torch
        from transformers import ClapFeatureExtractor, ClapModel
    except Exception as err:
        raise RuntimeError(
            "transformers and torch are required (pip install transformers torch)"
        ) from err

    feature_extractor = ClapFeatureExtractor.from_pretrained(args.model)
    model = ClapModel.from_pretrained(args.model)
    model.eval()

    samples, sr = load_audio(args.audio_path, feature_extractor.sampling_rate)
    inputs = feature_extractor(
        samples,
        sampling_rate=sr,
        return_tensors="pt",
    )
    with torch.no_grad():
        outputs = model.get_audio_features(**inputs)

    embedding = outputs[0].cpu().numpy().astype("float32")
    dim = int(embedding.shape[0])
    first8 = embedding[:8].tolist()
    norm = math.sqrt(float((embedding * embedding).sum()))

    print(f"dim: {dim}")
    print("first8:", ", ".join(f"{v:.6f}" for v in first8))
    print(f"l2_norm: {norm:.6f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
