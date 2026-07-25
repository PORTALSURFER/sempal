thread_local! {
    static STUB_STARTUP_AUDIO_REFRESH_FOR_TESTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static STARTUP_AUDIO_REFRESH_COUNT_FOR_TESTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Stub startup audio probing so tests can assert the deferral boundary without touching drivers.
pub(crate) fn with_stubbed_startup_audio_refresh_for_tests<T>(run: impl FnOnce() -> T) -> T {
    struct Reset<'a> {
        enabled: &'a std::cell::Cell<bool>,
        previous_enabled: bool,
        count: &'a std::cell::Cell<usize>,
        previous_count: usize,
    }

    impl Drop for Reset<'_> {
        fn drop(&mut self) {
            self.count.set(self.previous_count);
            self.enabled.set(self.previous_enabled);
        }
    }

    STUB_STARTUP_AUDIO_REFRESH_FOR_TESTS.with(|enabled| {
        STARTUP_AUDIO_REFRESH_COUNT_FOR_TESTS.with(|count| {
            let _reset = Reset {
                enabled,
                previous_enabled: enabled.replace(true),
                count,
                previous_count: count.replace(0),
            };
            run()
        })
    })
}

/// Return how many deferred startup audio refreshes were requested in the current test scope.
pub(crate) fn startup_audio_refresh_count_for_tests() -> usize {
    STARTUP_AUDIO_REFRESH_COUNT_FOR_TESTS.with(std::cell::Cell::get)
}

/// Record a stubbed startup audio refresh and return whether probing should be skipped.
pub(crate) fn record_startup_audio_refresh_for_tests() -> bool {
    if !STUB_STARTUP_AUDIO_REFRESH_FOR_TESTS.with(std::cell::Cell::get) {
        return false;
    }
    STARTUP_AUDIO_REFRESH_COUNT_FOR_TESTS.with(|count| {
        count.set(count.get().saturating_add(1));
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_audio_stub_restores_nested_state_after_unwind() {
        with_stubbed_startup_audio_refresh_for_tests(|| {
            assert!(record_startup_audio_refresh_for_tests());
            assert_eq!(startup_audio_refresh_count_for_tests(), 1);

            let unwind = std::panic::catch_unwind(|| {
                with_stubbed_startup_audio_refresh_for_tests(|| {
                    assert_eq!(startup_audio_refresh_count_for_tests(), 0);
                    assert!(record_startup_audio_refresh_for_tests());
                    panic!("exercise startup audio test scope cleanup");
                });
            });
            assert!(unwind.is_err());
            assert_eq!(startup_audio_refresh_count_for_tests(), 1);
        });

        assert!(!record_startup_audio_refresh_for_tests());
        assert_eq!(startup_audio_refresh_count_for_tests(), 0);
    }
}
