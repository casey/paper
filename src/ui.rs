//! Implements the interface between the user and the application.
//!
//! The primary output functionality of the user interface is to show the text of a document. Additionally, the user interface may show a message to the user. A message will overlap the upper portion of the document until it has been cleared.
use {
    clap::ArgMatches,
    core::{cmp, convert::TryInto, time::Duration},
    crossterm::{
        cursor::MoveTo,
        event::{self, Event},
        execute, queue,
        style::{Color, Print, ResetColor, SetBackgroundColor},
        terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
        ErrorKind,
    },
    log::{trace, warn},
    lsp_types::{MessageType, ShowMessageParams, ShowMessageRequestParams, TextEdit},
    std::io::{self, Stdout, Write},
};

/// The [`Err`] value returned by this module.
pub(crate) type Error = ErrorKind;

/// The user interface provided by a terminal.
#[derive(Debug)]
pub(crate) struct Terminal {
    /// The output of the application.
    out: Stdout,
    /// If `Terminal` has been initialized.
    is_init: bool,
    /// Inputs from command arguments.
    ///
    /// Command arguments are viewed as input so that all processing of arguments is performed within the main application loop.
    arg_inputs: Vec<Config>,
    /// Number of columns provided by terminal.
    columns: u16,
    /// Number of rows provided by terminal.
    rows: u16,
    /// The index of the first line of the document that may be displayed.
    first_line: u64,
    /// The grid of `chars` that represent the terminal.
    grid: Vec<String>,
    /// The number of lines currrently covered by an alert.
    alert_line_count: usize,
}

impl Terminal {
    /// Initializes the terminal user interface.
    pub(crate) fn init(&mut self, settings: Settings) -> crossterm::Result<()> {
        if let Some(file) = settings.file {
            self.arg_inputs.push(Config::File(file))
        }

        // Store all previous terminal output.
        execute!(self.out, EnterAlternateScreen)?;
        self.is_init = true;
        Ok(())
    }

    /// Applies `change` to the output.
    pub(crate) fn apply(&mut self, change: Change) -> crossterm::Result<()> {
        match change {
            Change::Text(edits) => {
                for edit in edits {
                    let start_row = self.get_row(edit.range.start.line);
                    let end_row = self.get_row(edit.range.end.line);
                    let mut modifications = self.get_modifications(&edit);

                    self.print_at_row(start_row, &modifications.join("\n"), None)?;

                    if let Some(modified_lines) =
                        self.grid.get_mut(start_row.into()..=end_row.into())
                    {
                        modified_lines.swap_with_slice(&mut modifications);
                    }
                }
            }
            Change::Message(alert) => {
                trace!("alert: {:?} {}", alert.typ, alert.message);
                self.alert_line_count = alert.message.lines().count();
                self.print_at_row(0, &alert.message, Some(alert.typ))?;
            }
            Change::Question(question) => {
                self.alert_line_count = question.message.lines().count();
                self.print_at_row(0, &question.message, Some(question.typ))?;
            }
            Change::Reset => {
                if self.alert_line_count != 0 {
                    self.print_at_row(
                        0,
                        &self
                            .grid
                            .get(0..self.alert_line_count)
                            .unwrap_or_default()
                            .join("\n"),
                        None,
                    )?;
                    self.alert_line_count = 0;
                }
            }
        }

        self.out.flush().map_err(Error::IoError)
    }

    /// Returns the row of `line` within the visible grid.
    ///
    /// `0` indicates `line` is either the first line of the grid or above it.
    /// `self.rows - 1` indicates `line` is either the last line of the grid or below it.
    fn get_row(&self, line: u64) -> u16 {
        cmp::min(
            line.saturating_sub(self.first_line)
                .try_into()
                .unwrap_or(u16::max_value()),
            self.rows.saturating_sub(1),
        )
    }

    /// Returns the lines within `edit` that will modify the user interface.
    fn get_modifications(&self, edit: &TextEdit) -> Vec<String> {
        edit.new_text
            .lines()
            .skip(
                self.first_line
                    .saturating_sub(edit.range.start.line)
                    .try_into()
                    .unwrap_or(usize::max_value()),
            )
            .take(self.rows.into())
            .map(String::from)
            .collect::<Vec<String>>()
    }

    /// Adds to the queue the commands to print `s` starting at column 0 of `row`.
    fn print_at_row(
        &mut self,
        row: u16,
        s: &str,
        context: Option<MessageType>,
    ) -> crossterm::Result<()> {
        let mut r = row;

        for line in s.lines() {
            queue!(self.out, MoveTo(0, r))?;

            if let Some(t) = context {
                queue!(
                    self.out,
                    SetBackgroundColor(match t {
                        MessageType::Error => Color::Red,
                        MessageType::Warning => Color::Yellow,
                        MessageType::Info => Color::Blue,
                        MessageType::Log => Color::Grey,
                    })
                )?;
            }

            queue!(self.out, Print(line), Clear(ClearType::UntilNewLine))?;

            if context.is_some() {
                queue!(self.out, ResetColor)?;
            }

            r = r.saturating_add(1);
        }

        Ok(())
    }

    /// Returns the input from the user.
    ///
    /// First checks for arg inputsReturns [`None`] if no input is provided.
    pub(crate) fn input(&mut self) -> crossterm::Result<Option<Input>> {
        // First check arg inputs, then check for key input.
        match self.arg_inputs.pop() {
            Some(input) => Ok(Some(Input::Config(input))),
            None => Ok(if event::poll(Duration::from_secs(0))? {
                Some(Input::User(event::read()?))
            } else {
                None
            }),
        }
    }
}

impl Default for Terminal {
    fn default() -> Self {
        let (columns, rows) = terminal::size().unwrap_or_default();

        let mut grid = Vec::default();

        for _ in 0..rows {
            grid.push(String::default());
        }

        Self {
            out: io::stdout(),
            is_init: false,
            arg_inputs: Vec::default(),
            columns,
            rows,
            first_line: 0,
            grid,
            alert_line_count: 0,
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.is_init && execute!(self.out, LeaveAlternateScreen).is_err() {
            warn!("Failed to leave alternate screen");
        }
    }
}

/// Signifies a potential modification to the output of the user interface.
///
/// It is not always true that a `Change` will require a modification of the user interface output. For example, if a range of the document that is not currently displayed is changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Change {
    /// Text of the current document was modified.
    Text(Vec<TextEdit>),
    /// Message will be displayed to the user.
    Message(ShowMessageParams),
    /// Message will ask question to user and get a response.
    Question(ShowMessageRequestParams),
    /// Message will be cleared.
    Reset,
}

/// Signifies settings of the application.
#[derive(Debug, Default)]
pub struct Settings {
    /// The file to be viewed.
    file: Option<String>,
}

impl From<ArgMatches<'_>> for Settings {
    #[must_use]
    fn from(value: ArgMatches<'_>) -> Self {
        Self {
            file: value.value_of("file").map(str::to_string),
        }
    }
}

/// Signifies a configuration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Config {
    /// The file path of the document.
    File(String),
}

/// Signifies input provided by the user.
#[derive(Clone, Debug)]
pub(crate) enum Input {
    /// User input.
    User(Event),
    /// Configuration.
    Config(Config),
}
