- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve DRYness, improve maintainability, collapse large structs/objects into clearly named smaller objects, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.
lets then write every task you find into @todo.md as a new todo item

--

\plan - lets tighten up the ui layout right now we have a bunch of double borders/frames in places, please redesign this, I want tight borders, section against section with only ever single borders. no double borders, insets, etc

### v2

\plan - add audio recording

\plan - add similarity search systems
let’s wire a tiny ONNX demon into our app. 
Here are concrete step-by-steps from “no CLAP” to “Rust calls ONNX model and gets embeddings”.

split into:
- One-time model export (Python side, dev only)
- Wiring ONNX Runtime into Rust
- Audio preprocessing in Rust
- Running inference and getting embeddings
- Integrating with your existing pipeline / DB
- Testing against the Python reference

You don’t have to do it all at once, but this is the full arc.

1. One-time: export CLAP to ONNX (Python)

This is purely a dev step. Users never see it.

1.1. Set up a small Python env

Create a venv, install dependencies (rough sketch):

python -m venv venv
source venv/bin/activate  # or .\venv\Scripts\activate on Windows

pip install torch torchaudio
# plus the CLAP repo you're using, e.g.:
# pip install laion-clap

1.2. Small export script

You’ll need to adapt to the exact CLAP repo you use, but conceptually:

import torch
from laion_clap import CLAP_Module  # example; adjust to actual lib

device = "cpu"

# 1. Load pretrained model
model = CLAP_Module(enable_fusion=False)
model.load_state_dict(torch.load("pretrained_model.pt", map_location=device))
model.eval().to(device)

# 2. Create a dummy input with correct shape
# You need to know what CLAP expects, e.g. [batch, channels, samples]
dummy = torch.randn(1, 1, 48000 * 10, device=device)  # 10s mono example

# 3. Export to ONNX
torch.onnx.export(
    model,
    dummy,
    "clap_audio.onnx",
    input_names=["audio"],
    output_names=["embedding"],
    dynamic_axes={
        "audio": {0: "batch"},  # possibly time dimension too, if model supports it
        "embedding": {0: "batch"},
    },
    opset_version=17,
)


Then test the ONNX model with onnxruntime in Python once (optional, but comforting).

The important bit you must learn from the Python side:

Exact input tensor shape CLAP expects (channels, samples, dtype, scale).

Any required preprocessing (e.g. waveform normalized to [-1, 1], specific sample rate, fixed length/padding).

Write that down; you’ll mirror it in Rust.

You now have:

clap_audio.onnx – this will be shipped with your app as a data file.

2. Add ONNX Runtime to your Rust project

Use the Rust bindings around ONNX Runtime. Example with the onnxruntime crate.

2.1. Add dependencies

In Cargo.toml:

[dependencies]
onnxruntime = { version = "0.0.14", features = ["ndarray"] } # version example
ndarray = "0.15"


(Version numbers are illustrative; adjust to whatever is current.)

You’ll also eventually want:

symphonia = { version = "0.5", features = ["wav", "flac", "mp3", "ogg"] }
# or similar for audio decoding

2.2. ClapEngine skeleton

Create a module, e.g. src/clap_engine.rs:

use std::path::Path;
use onnxruntime::{environment::Environment, session::Session, GraphOptimizationLevel};
use onnxruntime::ndarray::Array2;
use onnxruntime::tensor::OrtOwnedTensor;

pub struct ClapEngine {
    _env: Environment,     // keep it alive
    session: Session,      // the ONNX session
    dim: usize,            // output embedding dimension
}

impl ClapEngine {
    pub fn new(model_path: &Path) -> anyhow::Result<Self> {
        // 1. Create environment
        let env = Environment::builder()
            .with_name("clap")
            .with_log_level(onnxruntime::LoggingLevel::Warning)
            .build()?;

        // 2. Create session
        let session = env
            .new_session_builder()?
            .with_optimization_level(GraphOptimizationLevel::All)?
            .with_model_from_file(model_path)?;

        // (optional) infer embedding dim from model metadata, or hardcode for now
        let dim = 512; // example

        Ok(Self { _env: env, session, dim })
    }

    pub fn embed_batch(&self, batch: &Array2<f32>) -> anyhow::Result<Vec<Vec<f32>>> {
        // batch shape: [B, N] or [B, C*N] depending on your design
        // You may need Array3 or Array4 depending on model input (B,C,T).

        let input_tensor_values = vec![batch.clone()];

        let outputs: Vec<OrtOwnedTensor<f32, _>> = self
            .session
            .run(input_tensor_values)?;

        let output = &outputs[0];
        let view = output.view(); // e.g. shape [B, dim]

        let mut result = Vec::with_capacity(view.shape()[0]);
        for row in view.outer_iter() {
            result.push(row.to_vec());
        }

        Ok(result)
    }
}


This is deliberately simplified; you will adjust shapes once you know the exact model input.

3. Audio preprocessing in Rust

You need a function that:

takes a file path (or already-decoded samples),

decodes audio,

resamples + converts to expected format,

returns a Vec<f32> (or ndarray) ready to feed into CLAP.

3.1. Decode file → mono PCM

Rough sketch using symphonia:

use symphonia::core::{
    audio::Signal,
    codecs::DecoderOptions,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use std::fs::File;
use std::path::Path;

pub fn load_audio_mono_f32(path: &Path, target_sr: u32) -> anyhow::Result<Vec<f32>> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec.is_some())
        .ok_or_else(|| anyhow::anyhow!("no supported audio tracks"))?;
    let codec_params = &track.codec_params;

    let mut decoder = symphonia::default::get_codecs().make(
        codec_params,
        &DecoderOptions::default(),
    )?;

    let mut samples = Vec::<f32>::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track.id {
            continue;
        }

        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let chans = spec.channels.count();

        // Convert to f32 mono
        let mut buf = vec![0.0f32; decoded.frames()];
        for (i, frame) in decoded.chan(0).iter().enumerate() {
            let mut acc = *frame as f32;
            // average channels if more than one
            for c in 1..chans {
                acc += decoded.chan(c)[i] as f32;
            }
            buf[i] = acc / chans as f32;
        }

        samples.extend(buf);
    }

    // TODO: resample to target_sr (using rubato or similar)
    // For now assume it's already at target_sr and just return.
    Ok(samples)
}


Later:

plug in a resampler (e.g. rubato) if the CLAP model expects a fixed sample rate.

trim / pad to a fixed length (e.g. first 10 seconds).

3.2. Build the input tensor

Suppose CLAP expects [batch, 1, samples].

Then:

use onnxruntime::ndarray::Array3;

pub fn build_clap_input(samples: &[f32], fixed_len: usize) -> Array3<f32> {
    let mut buf = vec![0.0f32; fixed_len];

    let len = samples.len().min(fixed_len);
    buf[..len].copy_from_slice(&samples[..len]);

    // Shape: [1, 1, fixed_len]
    Array3::from_shape_vec((1, 1, fixed_len), buf).unwrap()
}


(In ClapEngine::embed_batch you’d then expect Array3<f32> instead of Array2<f32>.)

4. Running inference & getting embeddings

Put it all together in a convenience function:

use std::path::Path;

impl ClapEngine {
    pub fn embed_file(&self, path: &Path) -> anyhow::Result<Vec<f32>> {
        let target_sr = 48000;
        let fixed_len = target_sr as usize * 10; // 10 seconds

        let mono = load_audio_mono_f32(path, target_sr)?;
        let input = build_clap_input(&mono, fixed_len); // Array3 [1,1,T]

        let outputs: Vec<OrtOwnedTensor<f32, _>> = self
            .session
            .run(vec![input])?;

        let output = &outputs[0];
        let view = output.view(); // expect [1, dim]
        let row = view.index_axis(onnxruntime::ndarray::Axis(0), 0);
        let mut emb = row.to_vec();

        // Optional: L2 normalize embedding
        let norm = (emb.iter().map(|x| x * x).sum::<f32>()).sqrt();
        if norm > 0.0 {
            for v in &mut emb {
                *v /= norm;
            }
        }

        Ok(emb)
    }
}


Now your app can do:

let engine = ClapEngine::new(Path::new("clap_audio.onnx"))?;
let emb = engine.embed_file(Path::new("some_sample.wav"))?;
println!("embedding len = {}", emb.len());


That’s the core.

5. Integrating with your existing pipeline

From here, wire into your sample library system:

In your indexing / analysis worker:

 For every new or changed file:

call engine.embed_file(path)

store emb in sample_embeddings table or .emb_index file

add to ANN index.

In your UI:

“Find similar”:

load embedding from DB

run ANN search

show results.

You already sketched that part earlier; now you have the engine.

6. Testing against the Python CLAP reference

Before trusting the Rust ONNX path, sanity-check:

Choose a few test files.

In Python:

Run them through the original CLAP model.

Save embeddings to .npy or .json.

In Rust:

Run the same files through your ONNX path.

Compare embeddings:

check cosine similarity between Python and Rust outputs.

You want cosine similarity very close to 1 (within numerical noise). If not, your preprocessing or shapes don’t match and we fix that.

That’s the full step-by-step:

one-time Python export to ONNX,

Rust ONNX Runtime integration,

audio → tensor preprocessing,

embeddings into your pipeline.

Once this works, everything else (ANN index, DB integration, UI) becomes just normal Rust app work, not ML witchcraft.


---