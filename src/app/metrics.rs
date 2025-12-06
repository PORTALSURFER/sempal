use std::sync::OnceLock;
use std::time::Instant;

fn enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var("SEMPAL_PROFILE").as_deref() == Ok("1"))
}

/// Profile a closure when SEMPAL_PROFILE=1, otherwise run it directly.
pub fn profile<T>(label: &str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let out = f();
    let elapsed = start.elapsed();
    eprintln!("[profile] {label}: {:.2}ms", elapsed.as_secs_f64() * 1000.0);
    out
}
