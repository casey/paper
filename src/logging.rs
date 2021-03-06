//! Implements the logging functionality of `paper`.
use {
    clap::ArgMatches,
    fehler::throws,
    log::{info, LevelFilter, Log, Metadata, Record, SetLoggerError},
    std::{
        fs::File,
        io::{self, Write},
        sync::{Arc, RwLock},
    },
    thiserror::Error,
    time::OffsetDateTime,
};

/// Creates a [`Logger`] and initializes it as the logger.
#[throws(InitLoggerError)]
pub(crate) fn init(config: LogConfig) {
    let logger = Logger::new(config.is_starship_enabled)?;

    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(config.level);
    info!("Logger initialized");
}

/// Records all logs generated by the application.
struct Logger {
    /// The file where logs shall be recorded.
    file: Arc<RwLock<File>>,
    /// If logs from [`starship`] shall be recorded.
    is_starship_enabled: bool,
}

impl Logger {
    /// Creates a new [`Logger`].
    #[throws(CreateLoggerError)]
    fn new(is_starship_enabled: bool) -> Self {
        let log_filename = "paper.log".to_string();

        Self {
            file: Arc::new(RwLock::new(File::create(&log_filename).map_err(
                |error| CreateLoggerError {
                    file: log_filename,
                    error,
                },
            )?)),
            is_starship_enabled,
        }
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        if metadata.target().starts_with("starship") {
            self.is_starship_enabled
        } else {
            true
        }
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            if let Ok(mut file) = self.file.write() {
                #[allow(unused_must_use)] // Log::log() does not propagate errors.
                {
                    writeln!(
                        file,
                        "{} [{}]: {}",
                        OffsetDateTime::now_local().format("%F %T"),
                        record.level(),
                        record.args()
                    );
                }
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.write() {
            #[allow(unused_must_use)] // Log::flush() does not propagate errors.
            {
                file.flush();
            }
        }
    }
}

/// The configuration of the application logger.
#[derive(Clone, Copy, Debug)]
pub struct LogConfig {
    /// If logs from starship shall be written.
    is_starship_enabled: bool,
    /// The minimum level of logs that shall be written.
    level: LevelFilter,
}

impl Default for LogConfig {
    #[inline]
    fn default() -> Self {
        Self {
            level: LevelFilter::Warn,
            is_starship_enabled: false,
        }
    }
}

impl From<&ArgMatches<'_>> for LogConfig {
    #[inline]
    fn from(value: &ArgMatches<'_>) -> Self {
        Self {
            level: match value.occurrences_of("verbose") {
                0 => LevelFilter::Warn,
                1 => LevelFilter::Info,
                2 => LevelFilter::Debug,
                _ => LevelFilter::Trace,
            },
            is_starship_enabled: value.value_of("log") == Some("starship"),
        }
    }
}

/// An error initializing the logger.
#[derive(Debug, Error)]
pub enum InitLoggerError {
    /// An error creating the logger.
    #[error(transparent)]
    Create(#[from] CreateLoggerError),
    /// An error setting the logger.
    #[error("unable to set logger: {0}")]
    Set(#[from] SetLoggerError),
}

/// An error creating a [`Logger`].
#[derive(Debug, Error)]
#[error("unable to create log file `{file}`: {error}")]
pub struct CreateLoggerError {
    /// The path of the log file.
    file: String,
    /// The error.
    #[source]
    error: io::Error,
}
