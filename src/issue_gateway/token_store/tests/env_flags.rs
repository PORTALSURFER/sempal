use super::*;

/// Security toggles must only accept strict `1`/`true` tokens.
#[test]
fn strict_env_var_truthy_only_accepts_one_and_true() {
    let mut runtime = env_lock();
    /// Test-only env var for strict token parsing behavior.
    const STRICT_ENV: &str = "WAVECRATE_TOKEN_STORE_STRICT_PARSE_TEST";
    runtime.remove_var(STRICT_ENV);
    assert!(!fallback_policy::env_var_truthy(STRICT_ENV));

    runtime.set_var(STRICT_ENV, "1");
    assert!(fallback_policy::env_var_truthy(STRICT_ENV));

    runtime.set_var(STRICT_ENV, "TrUe");
    assert!(fallback_policy::env_var_truthy(STRICT_ENV));

    runtime.set_var(STRICT_ENV, "on");
    assert!(!fallback_policy::env_var_truthy(STRICT_ENV));

    runtime.set_var(STRICT_ENV, "yes");
    assert!(!fallback_policy::env_var_truthy(STRICT_ENV));

    runtime.set_var(STRICT_ENV, "true ");
    assert!(!fallback_policy::env_var_truthy(STRICT_ENV));
}
