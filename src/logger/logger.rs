use anyhow::{Result, anyhow};
use clap::ValueEnum;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::sync::atomic::{AtomicUsize, Ordering};

static LOGGER: SimpleLogger = SimpleLogger {
    level: AtomicUsize::new(LEVEL_INFO),
};

const LEVEL_OFF: usize = 0;
const LEVEL_ERROR: usize = 1;
const LEVEL_WARN: usize = 2;
const LEVEL_INFO: usize = 3;
const LEVEL_DEBUG: usize = 4;
const LEVEL_TRACE: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum LogLevelArg {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevelArg {
    pub fn as_filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::Error,
            Self::Warn => LevelFilter::Warn,
            Self::Info => LevelFilter::Info,
            Self::Debug => LevelFilter::Debug,
            Self::Trace => LevelFilter::Trace,
        }
    }
}

struct SimpleLogger {
    level: AtomicUsize,
}

impl Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= usize_to_level_filter(self.level.load(Ordering::Relaxed))
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{}",
                format_log_line(record.level(), record.target(), &record.args().to_string())
            );
        }
    }

    fn flush(&self) {}
}

/// Initializes process-wide logging when the server is started with logging enabled.
pub fn init_logging(enabled: bool, level: LogLevelArg) -> Result<()> {
    if !enabled {
        log::set_max_level(LevelFilter::Off);
        return Ok(());
    }

    let filter = level.as_filter();
    LOGGER
        .level
        .store(level_filter_to_usize(filter), Ordering::Relaxed);

    match log::set_logger(&LOGGER) {
        Ok(()) => {
            log::set_max_level(filter);
            Ok(())
        }
        Err(err) => Err(anyhow!("init logger failed: {err}")),
    }
}

fn format_log_line(level: Level, target: &str, message: &str) -> String {
    format!("[{level} {target}] {message}")
}

fn level_filter_to_usize(level: LevelFilter) -> usize {
    match level {
        LevelFilter::Off => LEVEL_OFF,
        LevelFilter::Error => LEVEL_ERROR,
        LevelFilter::Warn => LEVEL_WARN,
        LevelFilter::Info => LEVEL_INFO,
        LevelFilter::Debug => LEVEL_DEBUG,
        LevelFilter::Trace => LEVEL_TRACE,
    }
}

fn usize_to_level_filter(level: usize) -> LevelFilter {
    match level {
        LEVEL_ERROR => LevelFilter::Error,
        LEVEL_WARN => LevelFilter::Warn,
        LEVEL_INFO => LevelFilter::Info,
        LEVEL_DEBUG => LevelFilter::Debug,
        LEVEL_TRACE => LevelFilter::Trace,
        _ => LevelFilter::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_arg_maps_to_log_filter() {
        assert_eq!(LogLevelArg::Error.as_filter(), LevelFilter::Error);
        assert_eq!(LogLevelArg::Warn.as_filter(), LevelFilter::Warn);
        assert_eq!(LogLevelArg::Info.as_filter(), LevelFilter::Info);
        assert_eq!(LogLevelArg::Debug.as_filter(), LevelFilter::Debug);
        assert_eq!(LogLevelArg::Trace.as_filter(), LevelFilter::Trace);
    }

    #[test]
    fn log_line_includes_level_target_and_message() {
        assert_eq!(
            format_log_line(Level::Info, "server", "started"),
            "[INFO server] started"
        );
    }
}
