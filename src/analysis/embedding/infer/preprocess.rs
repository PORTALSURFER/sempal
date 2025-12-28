use super::backend_io::run_panns_inference_for_model;
use super::EmbeddingBatchInput;
use super::PANNS_LOGMEL_LEN;
use crate::analysis::embedding::PannsModel;
use crate::analysis::embedding::logmel::prepare_panns_logmel;

pub(super) fn infer_embedding_with_model(
    model: &mut PannsModel,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    let input_slice = model.input_scratch.as_mut_slice();
    prepare_panns_logmel(
        &mut model.resample_scratch,
        &mut model.wave_scratch,
        &mut model.preprocess,
        input_slice,
        samples,
        sample_rate,
    )?;
    let mut embeddings = run_panns_inference_for_model(model, model.input_scratch.as_slice(), 1)?;
    embeddings
        .pop()
        .ok_or_else(|| "PANNs embedding output missing".to_string())
}

pub(super) fn infer_embeddings_with_model(
    model: &mut PannsModel,
    inputs: &[EmbeddingBatchInput<'_>],
) -> Result<Vec<Vec<f32>>, String> {
    let batch = inputs.len();
    let total_len = batch * PANNS_LOGMEL_LEN;
    model.input_batch_scratch.clear();
    model.input_batch_scratch.resize(total_len, 0.0);
    for (idx, input) in inputs.iter().enumerate() {
        let start = idx * PANNS_LOGMEL_LEN;
        let end = start + PANNS_LOGMEL_LEN;
        let out = &mut model.input_batch_scratch[start..end];
        prepare_panns_logmel(
            &mut model.resample_scratch,
            &mut model.wave_scratch,
            &mut model.preprocess,
            out,
            input.samples,
            input.sample_rate,
        )?;
    }
    run_panns_inference_for_model(model, model.input_batch_scratch.as_slice(), batch)
}
