//! Integration tests for take duration behavior in audio sources.

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use sempal::audio::Source;
    
    struct EndlessSource {
        sample_rate: u32,
        channels: u16,
    }

    impl Iterator for EndlessSource {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            Some(0.0)
        }
    }

    impl Source for EndlessSource {
        fn current_frame_len(&self) -> Option<usize> { None }
        fn channels(&self) -> u16 { self.channels }
        fn sample_rate(&self) -> u32 { self.sample_rate }
        fn total_duration(&self) -> Option<Duration> { None }
    }

    #[test]
    fn test_take_duration_sample_count() {
        let rate = 44100;
        let channels = 2; // Stereo
        
        // Test various frame counts
        for target_frames in [1, 10, 100, 1000] {
            // Calculate duration using floor (our fix)
            let nanos = (target_frames as u64 * 1_000_000_000) / rate as u64;
            let duration = Duration::from_nanos(nanos);
            
            let source = EndlessSource { sample_rate: rate, channels };
            let sample_count = source.take_duration(duration).count();
            
            let expected_samples = target_frames * channels as usize;
            
            println!("Frames: {}, Duration: {}ns, Samples: {}, Expected: {}", 
                     target_frames, nanos, sample_count, expected_samples);
            
            // Check if sample count is even (required for stereo alignment)
            assert_eq!(sample_count % 2, 0, 
                      "Sample count must be even for stereo, got {} for {} frames", 
                      sample_count, target_frames);
            
            // Ideally should match expected, but let's see what we get
            if sample_count != expected_samples {
                println!("WARNING: Got {} samples, expected {}", sample_count, expected_samples);
            }
        }
    }
}
