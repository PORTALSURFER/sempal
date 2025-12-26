use super::{HdbscanConfig, HdbscanMethod};

pub fn validate_request(
    method: HdbscanMethod,
    umap_version: Option<&str>,
    config: HdbscanConfig,
) -> Result<(), String> {
    if config.min_cluster_size == 0 {
        return Err("min_cluster_size must be greater than zero".to_string());
    }
    if let Some(min_samples) = config.min_samples {
        if min_samples == 0 {
            return Err("min_samples must be greater than zero".to_string());
        }
    }
    if method == HdbscanMethod::Umap {
        let version = umap_version.unwrap_or("");
        if version.is_empty() {
            return Err("Layout version required".to_string());
        }
    }
    Ok(())
}

pub fn ensure_non_empty(data: &[Vec<f32>]) -> Result<(), String> {
    if data.is_empty() {
        Err("No data points found for clustering".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_input() {
        assert!(ensure_non_empty(&[]).is_err());
    }

    #[test]
    fn rejects_invalid_config() {
        let config = HdbscanConfig {
            min_cluster_size: 0,
            min_samples: None,
            allow_single_cluster: false,
        };
        assert!(validate_request(HdbscanMethod::Embedding, None, config).is_err());
    }
}
