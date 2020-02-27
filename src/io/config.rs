//! Implements [`Consumer`] for configs.
use {
    crate::io::Input,
    core::{cell::Cell, fmt, time::Duration},
    log::{warn, LevelFilter},
    market::{Consumer, Producer, Queue, StdConsumer},
    notify::{DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher},
    serde::Deserialize,
    std::{fs, path::PathBuf, sync::mpsc},
    thiserror::Error,
};

/// An error consuming change.
#[derive(Clone, Copy, Debug, Error)]
pub enum ConsumeChangeError {
    /// Consume.
    #[error("")]
    Consume(#[source] <Queue<Setting> as Consumer>::Error),
}

/// The Change Filter.
pub(crate) struct ChangeFilter {
    /// The deserialization of the config file.
    config: Cell<Config>,
    /// Watches for events on the config file.
    #[allow(dead_code)] // Must keep ownership of watcher.
    watcher: Option<RecommendedWatcher>,
    /// Receives events generated by `watcher`.
    file_event_drain: StdConsumer<DebouncedEvent>,
    /// The queue.
    queue: Queue<Setting>,
}

impl ChangeFilter {
    /// Creates a new [`ChangeFilter`].
    pub(crate) fn new(path: &PathBuf) -> Self {
        let (event_tx, event_rx) = mpsc::channel();

        let (watcher, config) = if path.is_file() {
            let watcher = match notify::watcher(event_tx, Duration::from_secs(0)) {
                Ok(mut w) => {
                    if let Err(error) = w.watch(path, RecursiveMode::NonRecursive) {
                        warn!("unable to watch config file: {}", error);
                    }

                    Some(w)
                }
                Err(error) => {
                    warn!("unable to create config file watcher: {}", error);
                    None
                }
            };

            (watcher, Config::read(path))
        } else {
            (None, Config::default())
        };

        Self {
            config: Cell::new(config),
            watcher,
            file_event_drain: event_rx.into(),
            queue: Queue::new(),
        }
    }

    /// Process the queue.
    fn process(&self) {
        while self.file_event_drain.can_consume() {
            if let Ok(Some(DebouncedEvent::Write(config_file))) =
                self.file_event_drain.optional_consume()
            {
                let new_config = Config::read(&config_file);

                if new_config.wrap != self.config.get().wrap {
                    let _ = self.queue.produce(Setting::Wrap(new_config.wrap));
                }

                if new_config.starship_log != self.config.get().starship_log {
                    let _ = self
                        .queue
                        .produce(Setting::StarshipLog(new_config.starship_log));
                }

                self.config.set(new_config);
            }
        }
    }
}

impl fmt::Debug for ChangeFilter {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ChangeFilter {{config: {:?}, file_event_drain: {:?}, queue: {:?}}}",
            self.config, self.file_event_drain, self.queue
        )
    }
}

impl Consumer for ChangeFilter {
    type Good = Input;
    type Error = ConsumeChangeError;

    fn can_consume(&self) -> bool {
        self.process();
        self.queue.can_consume()
    }

    fn consume(&self) -> Result<Self::Good, Self::Error> {
        self.process();
        self.queue
            .consume()
            .map(|setting| setting.into())
            .map_err(Self::Error::Consume)
    }
}

/// Returns the default wrap value.
const fn default_wrap() -> bool {
    false
}

/// Returns the default starship log value.
const fn default_starship_log() -> LevelFilter {
    LevelFilter::Off
}

/// The configuration of the application.
#[derive(Clone, Copy, Debug, Deserialize)]
struct Config {
    /// If documents shall wrap.
    #[serde(default = "default_wrap")]
    wrap: bool,
    /// The level filter of starship logs.
    #[serde(default = "default_starship_log")]
    starship_log: LevelFilter,
}

impl Config {
    /// Reads the config file into a [`Config`].
    fn read(config_file: &PathBuf) -> Self {
        match fs::read_to_string(config_file) {
            Err(error) => {
                warn!(
                    "Unable to read `{}`: {}",
                    config_file.to_string_lossy(),
                    error
                );
                Self::default()
            }
            Ok(config_text) => match toml::from_str(&config_text) {
                Err(error) => {
                    warn!("Unable to deserialize `{}`: {}", config_text, error);
                    Self::default()
                }
                Ok(config) => config,
            },
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wrap: default_wrap(),
            starship_log: default_starship_log(),
        }
    }
}

/// Signifies a configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    /// If the document shall wrap long text.
    Wrap(bool),
    /// The level at which starship records shall be logged.
    StarshipLog(LevelFilter),
}

impl From<Setting> for Input {
    #[inline]
    fn from(value: Setting) -> Self {
        Self::Config(value)
    }
}
