//! A terminal-based editor with goals to maximize simplicity and efficiency.
//!
//! This project is very much in an alpha state.
//!
//! Its features include:
//! - Modal editing (keys implement different functionality depending on the current mode).
//! - Extensive but relatively simple filter grammar that allows user to select any text.
//!
//! Future items on the Roadmap:
//! - Add more filter grammar.
//! - Implement suggestions for commands to improve user experience.
//! - Support Language Server Protocol.
//!
//! # Usage
//!
//! To use paper, install and run the binary. If you are developing a rust crate that runs paper,
//! then create and run an instance by calling the following:
//!
//! ```ignore
//! extern crate paper;
//!
//! use paper::Paper;
//!
//! fn main() {
//!     let mut paper = Paper::new();
//!
//!     paper.run();
//! }
//! ```

#![doc(html_root_url = "https://docs.rs/paper/0.1.0")]

extern crate regex;

mod ui;

use regex::Regex;
use std::cmp;
use std::fmt;
use std::fs;
use std::ops::{Add, AddAssign, SubAssign};
use ui::{Region, UserInterface, Address, Length};

/// The paper application.
#[derive(Debug, Default)]
pub struct Paper {
    /// User interface of the application.
    ui: UserInterface,
    /// Current mode of the application.
    mode: Mode,
    /// Data of the file being edited.
    view: View,
    /// Characters being edited to be analyzed by the application.
    sketch: String,
    /// Index of the first displayed line.
    first_line: usize,
    /// [`Region`]s of the view that match the current filter.
    ///
    /// [`Region`]: .struct.Region.html
    filter_regions: Vec<Region>,
    /// Path of the file being edited.
    path: String,
    /// If the view should be redrawn.
    is_dirty: bool,
    /// [`Mark`] of the cursor.
    ///
    /// [`Mark`]: .struct.Mark.html
    mark: Mark,
}

impl Paper {
    /// Creates a new paper application.
    ///
    /// # Examples
    /// ```ignore
    /// # use paper::Paper;
    /// let paper = Paper::new();
    /// ```
    pub fn new() -> Paper {
        Default::default()
    }

    /// Runs the application.
    ///
    /// # Examples
    /// ```ignore
    /// # use paper::Paper;
    /// let mut paper = Paper::new();
    /// paper.run();
    /// ```
    pub fn run(&mut self) {
        self.ui.init();

        'main: loop {
            let operations = self.mode.handle_input(self.ui.get_input());

            for operation in operations {
                match self.operate(operation) {
                    Some(Notice::Quit) => break 'main,
                    None => (),
                }
            }
        }

        self.ui.close();
    }

    /// Performs the given [`Operation`].
    ///
    /// [`Operation`]: enum.Operation.html
    fn operate(&mut self, op: Operation) -> Option<Notice> {
        match op {
            Operation::ExecuteCommand => {
                let re = Regex::new(r"(?P<command>.+?)(?:\s|$)").unwrap();
                let command = self.sketch.clone();

                match re.captures(&command) {
                    Some(caps) => match &caps["command"] {
                        "see" => {
                            let see_re = Regex::new(r"see\s*(?P<path>.*)").unwrap();
                            self.path = see_re.captures(&self.sketch).unwrap()["path"].to_string();
                            self.view = View::with_file(&self.path);
                            self.first_line = 0;
                        }
                        "put" => {
                            fs::write(&self.path, &self.view.data).unwrap();
                        }
                        "end" => return Some(Notice::Quit),
                        _ => {}
                    },
                    None => {}
                }
            }
            Operation::AddToSketch(c) => {
                match self.mark.add(c, &self.view) {
                    Edit::Backspace => {
                        self.ui.delete_back();
                        self.sketch.pop();
                    }
                    Edit::Wash => {
                        self.is_dirty = true;
                        self.sketch.clear();
                    }
                    Edit::Add => {
                        self.ui.insert_char(c);
                        self.sketch.push(c);
                    }
                }

                match self.mode.enhance(&self.sketch, &self.view.data) {
                    Some(Enhancement::FilterRegions(regions)) => {
                        // Clear filter background.
                        for line in 0..self.ui.window_height() {
                            self.ui.set_background(&Region::line(line), 0);
                        }

                        for region in regions.iter() {
                            self.ui.set_background(region, 1);
                        }

                        self.filter_regions = regions;
                    }
                    None => {}
                }

                self.move_mark();
            }
            Operation::AddToView(c) => {
                self.view.add(c, self.mark.index);

                if self.is_dirty {
                    self.write_view();
                    // write_view() moves cursor so move it back
                    self.move_mark();
                    self.is_dirty = false;
                }
            }
            Operation::ChangeMode(mode) => {
                self.mode = mode;

                match self.mode {
                    Mode::Display => {
                        self.write_view();
                    }
                    Mode::Command | Mode::Filter => {
                        self.mark.reset();
                        self.move_mark();
                        self.sketch.clear();
                    }
                    Mode::Action => {}
                    Mode::Edit => {
                        self.write_view();
                        self.move_mark();
                        self.sketch.clear();
                    }
                }
            }
            Operation::ScrollDown => {
                self.first_line = cmp::min(
                    self.first_line + self.scroll_height(),
                    self.view.data.lines().count() - 1,
                );
                self.write_view();
            }
            Operation::ScrollUp => {
                let movement = self.scroll_height();

                if self.first_line < movement {
                    self.first_line = 0;
                } else {
                    self.first_line -= movement;
                }
                self.write_view();
            }
            Operation::SetMark(edge) => {
                self.mark = Marker{region: self.filter_regions[0]}.generate_mark(edge, &self.view);
            }
        }

        None
    }

    /// Displays the view on the user interface.
    fn write_view(&mut self) {
        self.ui.clear();
        self.ui.calc_line_number_width(self.view.data.lines().count());

        for (index, line) in self.view.data.lines().skip(self.first_line).take(self.ui.window_height()).enumerate() {
            self.ui.set_line(index, self.first_line + index + 1, line);
        }
    }

    /// Returns the height used for scrolling.
    fn scroll_height(&self) -> usize {
        self.ui.window_height() / 4
    }

    /// Moves cursor match the address of the [`Mark`].
    fn move_mark(&self) {
        self.ui.move_to(self.mark.address);
    }
}

#[derive(Debug, Default)]
struct View {
    data: String,
}

impl View {
    fn with_file(filename: &String) -> View {
        View {
            data: fs::read_to_string(filename).unwrap().replace('\r', ""),
        }
    }

    fn line_length(&self, address: &Address) -> usize {
        self.data.lines().nth(address.row).unwrap().len()
    }

    fn add(&mut self, c: char, index: Index) {
        // Ignore the case where index is not valid.
        if let Ok(i) = index.to_usize() {
            match c {
                ui::BACKSPACE => {
                    self.data.remove(i);
                }
                _ => {
                    self.data.insert(i - 1, c);
                }
            }
        }
    }
}

/// Indicates a specific Address of a given Region.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
enum Edge {
    /// Indicates the first Address of the Region.
    Start,
    /// Indicates the last Address of the Region.
    End,
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Indicates changes to the sketch and view to be made.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
enum Edit {
    /// Removes the previous character from the sketch.
    Backspace,
    /// Clears the sketch and redraws the view.
    Wash,
    /// Adds the character to the view.
    Add,
}

impl fmt::Display for Edit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

struct Marker {
    region: Region,
}

impl Marker {
    fn generate_mark(&self, edge: Edge, view: &View) -> Mark {
        let mut address = self.region.start();

        if edge == Edge::End {
            address.column += self.length(view);
        }

        Mark::with_address(address, view)
    }

    /// Returns the number of characters included in the region of the marker.
    fn length(&self, view: &View) -> usize {
        let length = self.region.length();

        match length {
            ui::EOL => view.line_length(&self.region.start()),
            _ => length.to_usize(),
        }
    }
}

/// An address and its respective index in a view.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
struct Mark {
    /// Index in view that corresponds with mark.
    index: Index,
    /// Address of mark.
    address: Address,
}

impl Mark {
    /// Creates a new mark at the given address.
    fn with_address(address: Address, view: &View) -> Mark {
        Mark {
            address: address,
            index: Index::with_address(address, view),
        }
    }

    /// Resets mark to default values.
    fn reset(&mut self) {
        self.index = Default::default();
        self.address.reset();
    }

    /// Moves mark based on the added char and returns the appropriate Edit.
    fn add(&mut self, c: char, view: &View) -> Edit {
        match c {
            ui::BACKSPACE => {
                self.index -= 1;

                if self.address.column == 0 {
                    self.address.row -= 1;
                    self.address.column = view.line_length(&self.address);
                    return Edit::Wash;
                }

                self.address.column -= 1;
                Edit::Backspace
            }
            ui::ENTER => {
                self.index += 1;
                self.address.row += 1;
                self.address.column = 0;
                Edit::Wash
            }
            _ => {
                self.address.column += 1;
                self.index += 1;
                Edit::Add
            }
        }
    }
}

impl fmt::Display for Mark {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.address, self.index)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
struct Index(Option<usize>);

impl Index {
    fn with_address(address: Address, view: &View) -> Index {
        Index::with_row(address.row, view) + address.column
    }

    fn with_row(row: usize, view: &View) -> Index {
        match row {
            0 => Default::default(),
            _ => Index(view.data.match_indices(ui::ENTER).nth(row - 1).map(|x| x.0 + 1)),
        }
    }

    fn to_usize(&self) -> Result<usize, ()> {
        self.0.ok_or(())
    }
}

impl Add<usize> for Index {
    type Output = Index;

    fn add(self, other: usize) -> Index {
        Index(self.0.map(|x| x + other))
    }
}

impl SubAssign<usize> for Index {
    fn sub_assign(&mut self, other: usize) {
        self.0.map(|x| x - other);
    }
}

impl AddAssign<usize> for Index {
    fn add_assign(&mut self, other: usize) {
        self.0.map(|x| x + other);
    }
}

impl Default for Index {
    fn default() -> Index {
        Index(Some(0))
    }
}

impl fmt::Display for Index {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[{}]", match self.0 {
            None => String::from("None"),
            Some(i) => format!("{}", i),
        })
    }
}

/// Specifies a procedure based on user input to be executed by the application.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
enum Operation {
    /// Changes the mode.
    ChangeMode(Mode),
    /// Executes the command in the sketch.
    ExecuteCommand,
    /// Scrolls the view down by 1/4 of the window.
    ScrollDown,
    /// Scrolls the view up by 1/4 of the window.
    ScrollUp,
    /// Adds a character to the sketch.
    AddToSketch(char),
    /// Adds a character to the view.
    AddToView(char),
    /// Sets the mark to be an edge of the filtered regions.
    SetMark(Edge),
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Specifies a procedure to enhance the current sketch.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
enum Enhancement {
    /// Highlights specified regions.
    FilterRegions(Vec<Region>),
}

impl fmt::Display for Enhancement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Enhancement::FilterRegions(regions) => {
                write!(f, "FilterRegions [")?;

                for region in regions {
                    write!(f, "  {}", region)?;
                }

                write!(f, "]")
            }
        }
    }
}

/// Specifies the result of an Operation to be processed by the application.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
enum Notice {
    /// Ends the application.
    Quit,
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Specifies the functionality of the editor for a given state.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
enum Mode {
    /// Displays the current view.
    Display,
    /// Displays the current command.
    Command,
    /// Displays the current filter expression and highlights the characters that match the filter.
    Filter,
    /// Displays the highlighting that has been selected.
    Action,
    /// Displays the current view along with the current edits.
    Edit,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Default for Mode {
    fn default() -> Mode {
        Mode::Display
    }
}

impl Mode {
    /// Returns the operations to be executed based on user input.
    fn handle_input(&self, input: Option<char>) -> Vec<Operation> {
        let mut operations = Vec::new();

        match input {
            Some(c) => {
                match *self {
                    Mode::Display => match c {
                        '.' => operations.push(Operation::ChangeMode(Mode::Command)),
                        '#' | '/' => {
                            operations.push(Operation::ChangeMode(Mode::Filter));
                            operations.push(Operation::AddToSketch(c));
                        }
                        'j' => operations.push(Operation::ScrollDown),
                        'k' => operations.push(Operation::ScrollUp),
                        _ => {}
                    },
                    Mode::Command => match c {
                        ui::ENTER => {
                            operations.push(Operation::ExecuteCommand);
                            operations.push(Operation::ChangeMode(Mode::Display));
                        }
                        ui::ESC => operations.push(Operation::ChangeMode(Mode::Display)),
                        _ => operations.push(Operation::AddToSketch(c)),
                    },
                    Mode::Filter => match c {
                        ui::ENTER => operations.push(Operation::ChangeMode(Mode::Action)),
                        ui::ESC => operations.push(Operation::ChangeMode(Mode::Display)),
                        _ => operations.push(Operation::AddToSketch(c)),
                    },
                    Mode::Action => match c {
                        ui::ESC => operations.push(Operation::ChangeMode(Mode::Display)),
                        'i' => {
                            operations.push(Operation::SetMark(Edge::Start));
                            operations.push(Operation::ChangeMode(Mode::Edit));
                        }
                        'I' => {
                            operations.push(Operation::SetMark(Edge::End));
                            operations.push(Operation::ChangeMode(Mode::Edit));
                        }
                        _ => {}
                    },
                    Mode::Edit => match c {
                        ui::ESC => operations.push(Operation::ChangeMode(Mode::Display)),
                        _ => {
                            operations.push(Operation::AddToSketch(c));
                            operations.push(Operation::AddToView(c));
                        }
                    },
                }
            }
            None => {}
        }

        operations
    }

    /// Returns the Enhancement to be added.
    fn enhance(&self, sketch: &String, view: &String) -> Option<Enhancement> {
        match *self {
            Mode::Filter => {
                let re = Regex::new(r"#(?P<line>\d+)|/(?P<key>.+)").unwrap();
                let mut regions = Vec::new();

                match &re.captures(sketch) {
                    Some(captures) => {
                        if let Some(line) = captures.name("line") {
                            // Subtract 1 to match row.
                            line.as_str()
                                .parse::<usize>()
                                .map(|i| i - 1)
                                .ok()
                                .map(|row| {
                                    regions.push(Region::line(row));
                                });
                        }

                        if let Some(key) = captures.name("key") {
                            for (row, line) in view.lines().enumerate() {
                                for (key_index, key_match) in line.match_indices(key.as_str()) {
                                    regions.push(Region::with_address_length(Address::with_row_column(row, key_index), Length::from(key_match.len())));
                                }
                            }
                        }
                    }
                    None => {}
                }

                Some(Enhancement::FilterRegions(regions))
            }
            Mode::Display | Mode::Command | Mode::Action | Mode::Edit => None,
        }
    }
}
