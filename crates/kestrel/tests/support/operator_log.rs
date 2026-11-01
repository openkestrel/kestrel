use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::prelude::*;

use kestrel::domain::SessionId;

static TRACE_CAPTURES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Default)]
pub struct OperatorLog(Arc<Mutex<Vec<u8>>>);

/// SQLite worker threads need the same subscriber as the runtime tasks that issue their queries.
pub fn capturing() -> &'static OperatorLog {
    static LOG: OnceLock<OperatorLog> = OnceLock::new();

    LOG.get_or_init(|| {
        let log = OperatorLog::default();
        let writing = log.clone();
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(move || writing.clone())
                    .with_filter(tracing_subscriber::filter::dynamic_filter_fn(
                        |metadata, _| {
                            *metadata.level() <= tracing::Level::INFO
                                || TRACE_CAPTURES.load(Ordering::Relaxed) > 0
                        },
                    )),
            )
            .init();

        log
    })
}

impl OperatorLog {
    pub fn containing(&self, text: &str) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().expect("the log should not be poisoned"))
            .lines()
            .filter(|line| line.contains(text))
            .map(str::to_owned)
            .collect()
    }

    pub fn about(&self, session: SessionId) -> Vec<String> {
        let session = session.to_string();

        String::from_utf8_lossy(&self.0.lock().expect("the log should not be poisoned"))
            .lines()
            .filter(|line| line.contains(&session))
            .map(str::to_owned)
            .collect()
    }
}

impl io::Write for OperatorLog {
    fn write(&mut self, written: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("the log should not be poisoned")
            .extend_from_slice(written);

        Ok(written.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn capturing_trace() -> Trace {
    let log = capturing();
    let from = log.0.lock().expect("the log should not be poisoned").len();
    TRACE_CAPTURES.fetch_add(1, Ordering::Relaxed);
    Trace { log, from }
}

pub struct Trace {
    log: &'static OperatorLog,
    from: usize,
}

impl Trace {
    pub fn said(&self) -> String {
        let log = self.log.0.lock().expect("the log should not be poisoned");
        String::from_utf8_lossy(&log[self.from..]).into_owned()
    }
}

impl Drop for Trace {
    fn drop(&mut self) {
        TRACE_CAPTURES.fetch_sub(1, Ordering::Relaxed);
    }
}
