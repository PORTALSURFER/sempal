pub(crate) fn analysis_log_enabled() -> bool {
    std::env::var("SEMPAL_ANALYSIS_LOG_JOBS")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub(crate) fn analysis_log_queue_enabled() -> bool {
    std::env::var("SEMPAL_ANALYSIS_LOG_QUEUE")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub(crate) fn panic_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "Unknown panic payload".to_string()
    };
    let backtrace = std::backtrace::Backtrace::capture();
    format!("Analysis worker panicked: {message}\n{backtrace}")
}
