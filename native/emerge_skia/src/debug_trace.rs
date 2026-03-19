#[cfg(feature = "hover-trace")]
use std::sync::OnceLock;
#[cfg(feature = "hover-trace")]
use std::time::Instant;

#[cfg(feature = "hover-trace")]
static HOVER_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();
#[cfg(feature = "hover-trace")]
static HOVER_TRACE_START: OnceLock<Instant> = OnceLock::new();

#[cfg(feature = "hover-trace")]
pub(crate) fn enabled() -> bool {
    *HOVER_TRACE_ENABLED.get_or_init(|| {
        matches!(
            std::env::var("EMERGE_HOVER_TRACE").ok().as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        )
    })
}

#[cfg(feature = "hover-trace")]
pub(crate) fn emit(tag: &str, message: String) {
    if !enabled() {
        return;
    }

    let elapsed_ms = HOVER_TRACE_START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis();
    eprintln!("[hover-trace][{elapsed_ms:>6}ms][{tag}] {message}");
}

#[cfg(not(feature = "hover-trace"))]
#[allow(dead_code)]
pub(crate) fn enabled() -> bool {
    false
}

#[cfg(not(feature = "hover-trace"))]
#[allow(dead_code)]
pub(crate) fn emit(_tag: &str, _message: String) {}

macro_rules! hover_trace {
    ($tag:expr, $($arg:tt)*) => {
        #[cfg(feature = "hover-trace")]
        {
            $crate::debug_trace::emit($tag, format!($($arg)*));
        }
    };
}

pub(crate) use hover_trace;
