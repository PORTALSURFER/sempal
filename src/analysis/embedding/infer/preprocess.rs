use super::backend_io::run_panns_inference_for_model;

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
