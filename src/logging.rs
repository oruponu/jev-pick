use tracing::Subscriber;
use tracing_subscriber::{
    EnvFilter, Layer, filter::filter_fn, fmt::MakeWriter, layer::SubscriberExt,
    util::SubscriberInitExt,
};

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("RUST_LOG is not a valid logging filter.")]
    InvalidFilter,
    #[error("Logging could not be initialized.")]
    Initialization,
}

pub fn init(filter: &str) -> Result<(), LogError> {
    make_subscriber(filter, std::io::stderr)?
        .try_init()
        .map_err(|_| LogError::Initialization)
}

fn make_subscriber<W>(filter: &str, writer: W) -> Result<impl Subscriber + Send + Sync, LogError>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let environment = EnvFilter::try_new(filter).map_err(|_| LogError::InvalidFilter)?;
    // Dependency diagnostics may contain HTTP or interaction payloads. This
    // independent target filter cannot be lifted by RUST_LOG directives.
    let application_only = filter_fn(|metadata| {
        metadata.target() == "jev_pick" || metadata.target().starts_with("jev_pick::")
    });
    let output = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_filter(environment)
        .with_filter(application_only);
    Ok(tracing_subscriber::registry().with(output))
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use super::*;

    #[derive(Clone)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn verbose_logging_cannot_enable_dependency_payload_logging() {
        let buffer = Buffer(Arc::new(Mutex::new(Vec::new())));
        let output = buffer.0.clone();
        let subscriber = make_subscriber("trace", move || buffer.clone()).unwrap();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "jev_pick", "safe-operational-event");
            tracing::debug!(target: "jev_pick::jev", attempt = 2, "safe-attempt-event");
            tracing::error!(target: "serenity", "sensitive-dependency-event");
            tracing::info!(target: "reqwest", "sensitive-provider-event");
            tracing::warn!(target: "jev_pick_untrusted", "sensitive-prefix-event");
        });
        let bytes = output.lock().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("safe-operational-event"));
        assert!(text.contains("safe-attempt-event"));
        assert!(!text.contains("sensitive-"));
    }

    #[test]
    fn invalid_log_filter_returns_a_value_free_error() {
        let result = make_subscriber("jev_pick[span{field=sensitive-filter}=trace", io::sink);
        assert!(matches!(result, Err(LogError::InvalidFilter)));
    }
}
