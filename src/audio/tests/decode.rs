use super::support::{assert_fixture_decodes, fixtures};
use crate::waveform::WaveformRenderer;

#[test]
fn decode_handles_varied_sample_rates_and_channels() {
    let renderer = WaveformRenderer::new(24, 12);
    let specs = [
        fixtures::ToneSpec::new(8_000, 1, 0.25).with_pulse(fixtures::TonePulse {
            start_seconds: 0.0,
            duration_seconds: 0.05,
            amplitude: 0.9,
        }),
        fixtures::ToneSpec::new(48_000, 2, 1.2).with_pulse(fixtures::TonePulse {
            start_seconds: 0.9,
            duration_seconds: 0.1,
            amplitude: 0.6,
        }),
        fixtures::ToneSpec::new(11_025, 2, 0.5).with_pulse(fixtures::TonePulse {
            start_seconds: 0.4,
            duration_seconds: 0.05,
            amplitude: 0.75,
        }),
    ];

    for spec in specs {
        let fixture = fixtures::build_fixture(spec);
        assert_fixture_decodes(&renderer, fixture);
    }
}
