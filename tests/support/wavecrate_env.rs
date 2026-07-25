use std::path::PathBuf;
use wavecrate_library::test_runtime::TestRuntimeGuard;

pub struct WavecrateEnvGuard {
    _runtime: TestRuntimeGuard,
}

impl WavecrateEnvGuard {
    pub fn set_config_home(path: PathBuf) -> Self {
        let mut runtime = TestRuntimeGuard::acquire();
        runtime.set_var("WAVECRATE_CONFIG_HOME", path);
        Self { _runtime: runtime }
    }
}
