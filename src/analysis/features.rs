use serde::{Deserialize, Serialize};

/// Versioned analysis output persisted per sample.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct AnalysisFeaturesV1 {
    /// Feature vector layout version.
    pub(crate) version: u32,
    /// Time-domain features extracted from analysis-normalized audio.
    pub(crate) time_domain: super::time_domain::TimeDomainFeatures,
    /// Frequency-domain features extracted from analysis-normalized audio.
    pub(crate) frequency_domain: super::frequency_domain::FrequencyDomainFeatures,
}

impl AnalysisFeaturesV1 {
    /// Create a v1 feature vector combining time and frequency domain features.
    pub(crate) fn new(
        time_domain: super::time_domain::TimeDomainFeatures,
        frequency_domain: super::frequency_domain::FrequencyDomainFeatures,
    ) -> Self {
        Self {
            version: 1,
            time_domain,
            frequency_domain,
        }
    }
}
