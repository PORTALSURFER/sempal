//! Regression tests for audio duration edge cases.

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use sempal::audio::Source;
    
    // A dummy source that produces infinite stereo samples
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
    fn test_duration_truncation() {
        let rate = 44100;
        let channels = 2; // Stereo
        
        // Target: 1 frame.
        let target_frames = 1;
        
        // Old method: f32 -> Duration
        // 1.0 / 44100.0
        let frame_duration_f32 = 1.0 / rate as f32;
        let duration_f32 = Duration::from_secs_f32(frame_duration_f32);
        
        println!("F32 Duration: {:?}", duration_f32);
        println!("F32 Nanos: {}", duration_f32.as_nanos());

        let source = EndlessSource { sample_rate: rate, channels };
        let count_f32 = source.take_duration(duration_f32).count();
        
        println!("Samples with F32 duration: {}", count_f32);
        
        // Expected samples: 1 frame * 2 channels = 2 samples.
        // If count_f32 < 2, we have a problem (stereo swap if loop continues).
        
        // New method: u64 ceil
        let nanos = (target_frames as u64 * 1_000_000_000 + rate as u64 - 1) / rate as u64;
        let duration_u64 = Duration::from_nanos(nanos);
        
        println!("U64 Duration: {:?}", duration_u64);
        println!("U64 Nanos: {}", duration_u64.as_nanos());
        
        let source2 = EndlessSource { sample_rate: rate, channels };
        let count_u64 = source2.take_duration(duration_u64).count();
        
        println!("Samples with U64 duration: {}", count_u64);

        assert_eq!(count_u64, 2, "U64 method should yield exact samples");
    }

    #[test]
    fn test_skip_duration_precision() {
        let rate = 44100;
        
        // Target: skip 1 frame.
        let target_frames = 1;

        // Old method: f32 -> Duration
        let frame_duration_f32 = 1.0 / rate as f32;
        let skip_f32 = Duration::from_secs_f32(frame_duration_f32);
        
        println!("Skip F32: {:?}", skip_f32);
        
        // New method: u64 floor (integer truncation)
        // Based on analysis, we must not overshoot.
        let nanos = (target_frames as u64 * 1_000_000_000) / rate as u64;
        let skip_u64 = Duration::from_nanos(nanos);
        
        println!("Skip U64: {:?}", skip_u64);
        
        // Note: rodio::Source::skip_duration is provided by Source extension trait.
        // It consumes items until duration is reached.
        // We can't easily test rodio's implementation without rodio source, 
        // but we can trust that the same math applies: 
        // if duration < required time for 1 frame, it might skip 0 frames?
        // Or if duration >= time for 1 sample but < time for 2 samples (1 frame stereo),
        // it effectively skips 1 sample -> CHANNEL SWAP.
        
        // Let's verify the nanoseconds:
        // 1 frame @ 44100 = 22675.73 ns
        // f32 from_secs(1/44100) = 22675 ns (truncated integer math in std or f32 precision?)
        // 1 sample @ 44100 * 2ch = 88200 samples/sec? No, rate is frame rate.
        // Sub-sample time: 
        // Time for 1 sample (1/2 frame) = 1/(44100*2) = 11337.8 ns.
        // 22675 ns is > 11337.8 ns (1 sample) but < 22675.73 ns (2 samples).
        // So `skip_duration` sees enough time to skip 1 sample, but NOT 2 samples.
        // RESULT: Skips 1 sample. Channels SWAPPED.
        
        let precise_nanos = (1_000_000_000.0f64 / 44100.0f64) as u64; // 22675
        let _needed_nanos = ((1.0f64 / 44100.0f64) * 1_000_000_000.0f64).ceil() as u64; // 22676?
        
        println!("skip_f32: {}", skip_f32.as_nanos());
        println!("skip_u64: {}", skip_u64.as_nanos());
        println!("precise: {}", precise_nanos);
        
        // Check that F32 overshoots the precise floor value
        assert!(skip_f32.as_nanos() > skip_u64.as_nanos(), "F32 duration overshoots precise frame duration (causing swap)");
        
        // Check that U64 matches expected precise floor
        assert_eq!(skip_u64.as_nanos(), precise_nanos as u128, "U64 matches precise floor");
        
        // If skip_f32 is big enough for 1 sample (half frame) but not 2 samples (full frame),
        // then we have a swap.
        let half_frame_nanos = 1_000_000_000 / (rate as u128 * 2);
        assert!(skip_f32.as_nanos() > half_frame_nanos, "Skips at least one sample");
        
        // ASSERTION FOR FIX:
        // skip_u64 should be calculated using FLOOR (integer truncation).
        // 1 frame time = 22675.73...
        // skip_u64 should be 22675.
        // 22675 <= 22675.73. 
        // Rodio `skip_duration` stops when accumulated duration >= 22675?
        // Or strictly >?
        // If rodio loop: while skipped < requested.
        // Sample 0 (0 < 22675). Skipped (Value 11337).
        // Sample 1 (11337 < 22675). Skipped (Value 22675 approx).
        // Sample 2 (22675 < 22675). FALSE (assuming exact match).
        // So Sample 2 (Left of Frame 2) is NOT skipped.
        // We start exactly at Frame 2. Correct.
        
        // If we used CEIL (22676).
        // Sample 2 (22675 < 22676). TRUE.
        // Sample 2 IS skipped.
        // We start at Sample 3 (Right of Frame 2).
        // STEREO SWAP.
        
        assert_eq!(skip_u64.as_nanos(), precise_nanos as u128, "Should use floor logic to prevent overshoot");
    }
}
