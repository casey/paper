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

// Lint checks currently not defined: missing_doc_code_examples, variant_size_differences
#![warn(
    rust_2018_idioms,
    future_incompatible,
    unused,
    box_pointers,
    macro_use_extern_crate,
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    unsafe_code,
    unused_import_braces,
    unused_lifetimes,
    unused_qualifications,
    unused_results
)]
#![doc(html_root_url = "https://docs.rs/paper/0.1.0")]

mod engine;
mod ui;

use crate::engine::{Controller, Notice};
use crate::ui::{Address, Change, Color, Edit, Length, Region, UserInterface, END};
use rec::{Atom, ChCls, Pattern, SOME};
use std::cmp;
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter, Result as FmtResult};
use std::fs;
use std::iter::once;
use std::num::NonZeroUsize;
use std::ops::{Add, AddAssign, Shr, Sub, SubAssign};

/// The paper application.
#[derive(Debug, Default)]
pub struct Paper {
    /// User interface of the application.
    ui: UserInterface,
    controller: Controller,
    /// Data of the file being edited.
    view: View,
    /// Characters being edited to be analyzed by the application.
    sketch: String,
    /// [`Section`]s of the view that match the current filter.
    ///
    /// [`Section`]: .struct.Section.html
    signals: Vec<Section>,
    noises: Vec<Section>,
    marks: Vec<Mark>,
    filters: PaperFilters,
    sketch_additions: String,
}

impl Paper {
    /// Creates a new paper application.
    pub fn new() -> Paper {
        Default::default()
    }

    /// Runs the application.
    pub fn run(&mut self) -> Result<(), String> {
        self.ui.init()?;
        let operations = engine::Operations::default();

        'main: loop {
            for opcode in self.controller.process_input(self.ui.receive_input()) {
                match operations.execute(self, opcode)? {
                    Some(Notice::Quit) => break 'main,
                    Some(Notice::Flash) => {
                        self.ui.flash()?;
                    }
                    None => {}
                }
            }
        }

        self.ui.close()?;
        Ok(())
    }

    /// Displays the view on the user interface.
    fn display_view(&self) -> Result<(), String> {
        for edit in self.view.redraw_edits().take(self.ui.grid_height()) {
            self.ui.apply(edit)?;
        }

        Ok(())
    }

    fn change_view(&mut self, path: &str) {
        self.view = View::with_file(String::from(path));
        self.noises.clear();

        for line in 1..=self.view.line_count {
            // Safe to unwrap because line >= 1.
            self.noises
                .push(Section::line(LineNumber::new(line).unwrap()));
        }
    }

    fn save_view(&self) {
        self.view.put();
    }

    fn reduce_noise(&mut self) {
        self.noises = self.signals.clone();
    }

    fn filter_signals(&mut self, feature: &str) {
        self.signals = self.noises.clone();

        if let Some(id) = feature.chars().nth(0) {
            for filter in self.filters.iter() {
                if id == filter.id() {
                    filter.extract(feature, &mut self.signals, &self.view);
                    break;
                }
            }
        }
    }

    fn sketch(&self) -> &String {
        &self.sketch
    }

    fn add_to_sketch(&mut self, c: char) -> bool {
        match c {
            ui::BACKSPACE => {
                if let None = self.sketch.pop() {
                    return false;
                }
            }
            _ => {
                self.sketch.push(c);
            }
        }

        return true;
    }

    fn draw_popup(&self) -> Result<(), String> {
        self.ui
            .apply(Edit::new(Region::row(0), Change::Row(self.sketch.clone())))
    }

    fn clear_background(&self) -> Result<(), String> {
        for row in 0..self.ui.grid_height() {
            self.format_region(Region::row(row), Color::Default)?;
        }

        Ok(())
    }

    fn set_marks(&mut self, edge: &Edge) {
        self.marks.clear();

        for signal in self.signals.iter() {
            let mut place = signal.start;

            if *edge == Edge::End {
                let length = signal.length;

                place.index += match length {
                    END => self.view.line_length(&signal.start),
                    _ => length.to_usize(),
                };
            }

            self.marks.push(Mark {
                place,
                pointer: place.index
                    + Pointer(match place.line.index() {
                        0 => Some(0),
                        index => self
                            .view
                            .data
                            .match_indices(ui::ENTER)
                            .nth(index - 1)
                            .map(|x| x.0 + 1),
                    }),
            });
        }
    }

    fn scroll(&mut self, movement: isize) {
        self.view.scroll(movement);
    }

    fn draw_filter_backgrounds(&self) -> Result<(), String> {
        for noise in self.noises.iter() {
            self.format_section(noise, Color::Blue)?;
        }

        for signal in self.signals.iter() {
            self.format_section(signal, Color::Red)?;
        }

        Ok(())
    }

    fn format_section(&self, section: &Section, color: Color) -> Result<(), String> {
        if let Some(region) = section.to_region(&self.view.origin) {
            self.format_region(region, color)?;
        }

        // Region is not displayable, which is generally not an error.
        Ok(())
    }

    fn format_region(&self, region: Region, color: Color) -> Result<(), String> {
        self.ui.apply(Edit::new(region, Change::Format(color)))
    }

    fn reset_sketch(&mut self) {
        self.sketch.clear();
    }

    fn update_view(&mut self, c: char) -> Result<(), String> {
        let mut adjustment: Adjustment = Default::default();

        for mark in self.marks.iter_mut() {
            adjustment += Adjustment::create(c, &mark.place, &self.view);

            if adjustment.change != Change::Clear {
                if let Some(region) = mark.place.to_region(&self.view.origin) {
                    self.ui
                        .apply(Edit::new(region, adjustment.change.clone()))?;
                }
            }

            mark.adjust(&adjustment);
            self.view.add(mark, c);
        }

        if adjustment.change == Change::Clear {
            self.view.clean();
            self.display_view()?;
        }

        Ok(())
    }

    fn change_mode(&mut self, mode: engine::Mode) {
        self.controller.set_mode(mode);
    }

    /// Returns the height used for scrolling.
    fn scroll_height(&self) -> usize {
        self.ui.grid_height() / 4
    }
}


#[derive(Debug, Default)]
struct PaperFilters {
    line: LineFilter,
    pattern: PatternFilter,
}

impl PaperFilters {
    fn iter(&self) -> PaperFiltersIter<'_> {
        PaperFiltersIter {
            index: 0,
            filters: self,
        }
    }
}

struct PaperFiltersIter<'a> {
    index: usize,
    filters: &'a PaperFilters,
}

impl<'a> Iterator for PaperFiltersIter<'a> {
    type Item = &'a dyn Filter;

    fn next(&mut self) -> Option<Self::Item> {
        self.index += 1;

        match self.index {
            1 => Some(&self.filters.line),
            2 => Some(&self.filters.pattern),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct View {
    data: String,
    origin: RelativePlace,
    line_count: usize,
    path: String,
}

impl View {
    fn with_file(path: String) -> View {
        let mut view = View {
            data: fs::read_to_string(path.as_str()).unwrap().replace('\r', ""),
            path: path,
            ..Default::default()
        };

        view.clean();
        view
    }

    fn add(&mut self, mark: &Mark, c: char) {
        let index = mark.pointer.to_usize();

        match c {
            ui::BACKSPACE => {
                // For now, do not care to check what is removed. But this may become important for
                // multi-byte characters.
                match self.data.remove(index) {
                    _ => {}
                }
            }
            _ => {
                self.data.insert(index - 1, c);
            }
        }
    }

    fn redraw_edits(&self) -> impl Iterator<Item = Edit> + '_ {
        // Clear the screen, then add each row.
        once(Edit::new(Default::default(), Change::Clear)).chain(
            self.lines()
                .skip(self.origin.line.index())
                .enumerate()
                .map(move |x| {
                    Edit::new(
                        Region::row(x.0),
                        Change::Row(format!(
                            "{:>width$} {}",
                            self.origin.line + x.0,
                            x.1,
                            width = (-self.origin.index - 1) as usize
                        )),
                    )
                }),
        )
    }

    fn lines(&self) -> std::str::Lines<'_> {
        self.data.lines()
    }

    fn line(&self, line_number: LineNumber) -> Option<&str> {
        self.lines().nth(line_number.index())
    }

    fn clean(&mut self) {
        self.line_count = self.lines().count();
        self.origin.index = -(((self.line_count + 1) as f32).log10().ceil() as isize + 1);
    }

    fn scroll(&mut self, movement: isize) {
        self.origin.line = cmp::min(
            self.origin.line + movement,
            LineNumber::new(self.line_count).unwrap_or(Default::default()),
        );
    }

    fn line_length(&self, place: &Place) -> usize {
        self.line(place.line).unwrap().len()
    }

    fn put(&self) {
        fs::write(&self.path, &self.data).unwrap();
    }
}

#[derive(Clone, Debug, Default)]
struct Adjustment {
    shift: isize,
    line_change: isize,
    indexes_changed: HashMap<LineNumber, isize>,
    change: Change,
}

impl Adjustment {
    fn new(line: LineNumber, shift: isize, index_change: isize, change: Change) -> Adjustment {
        let line_change = if change == Change::Clear { shift } else { 0 };

        Adjustment {
            shift,
            line_change,
            indexes_changed: [(line + line_change, index_change)]
                .iter()
                .cloned()
                .collect(),
            change,
        }
    }

    fn create(c: char, place: &Place, view: &View) -> Adjustment {
        match c {
            ui::BACKSPACE => {
                if place.index == 0 {
                    Adjustment::new(
                        place.line,
                        -1,
                        view.line_length(place) as isize,
                        Change::Clear,
                    )
                } else {
                    Adjustment::new(place.line, -1, -1, Change::Backspace)
                }
            }
            ui::ENTER => Adjustment::new(place.line, 1, -(place.index as isize), Change::Clear),
            _ => Adjustment::new(place.line, 1, 1, Change::Insert(c)),
        }
    }
}

impl AddAssign for Adjustment {
    fn add_assign(&mut self, other: Adjustment) {
        self.shift += other.shift;
        self.line_change += other.line_change;

        for (line, change) in other.indexes_changed {
            *self.indexes_changed.entry(line).or_default() += change;
        }

        if self.change != Change::Clear {
            self.change = other.change
        }
    }
}

/// Indicates a specific Place of a given Section.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
enum Edge {
    /// Indicates the first Place of the Section.
    Start,
    /// Indicates the last Place of the Section.
    End,
}

impl Default for Edge {
    fn default() -> Edge {
        Edge::Start
    }
}

impl Display for Edge {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{:?}", self)
    }
}

/// An address and its respective pointer in a view.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
struct Mark {
    /// Pointer in view that corresponds with mark.
    pointer: Pointer,
    /// Place of mark.
    place: Place,
}

impl Mark {
    fn adjust(&mut self, adjustment: &Adjustment) {
        if -adjustment.shift < self.pointer.to_isize() {
            self.pointer += adjustment.shift;
            self.place.line = self.place.line + adjustment.line_change;

            for (&line, &change) in adjustment.indexes_changed.iter() {
                if line == self.place.line {
                    self.place.index = (self.place.index as isize + change) as usize;
                }
            }
        }
    }
}

impl Display for Mark {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}{}", self.place, self.pointer)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
struct Pointer(Option<usize>);

impl Pointer {
    fn to_usize(&self) -> usize {
        self.0.unwrap()
    }

    fn to_isize(&self) -> isize {
        self.0.unwrap() as isize
    }
}

impl Add<Pointer> for usize {
    type Output = Pointer;

    fn add(self, other: Pointer) -> Pointer {
        Pointer(other.0.map(|x| x + self))
    }
}

impl Add<usize> for Pointer {
    type Output = Pointer;

    fn add(self, other: usize) -> Pointer {
        Pointer(self.0.map(|x| x + other))
    }
}

impl SubAssign<usize> for Pointer {
    fn sub_assign(&mut self, other: usize) {
        self.0 = self.0.map(|x| x - other);
    }
}

impl AddAssign<isize> for Pointer {
    fn add_assign(&mut self, other: isize) {
        self.0 = self.0.map(|x| (x as isize + other) as usize);
    }
}

impl Default for Pointer {
    fn default() -> Pointer {
        Pointer(Some(0))
    }
}

impl Display for Pointer {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "[{}]",
            match self.0 {
                None => String::from("None"),
                Some(i) => format!("{}", i),
            }
        )
    }
}

/// Signifies adjacent [`Place`]s.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct Section {
    start: Place,
    length: Length,
}

impl Section {
    /// Creates a new `Section` that signifies an entire line.
    pub fn line(line: LineNumber) -> Section {
        Section {
            start: Place { line, index: 0 },
            length: END,
        }
    }

    fn to_region(&self, origin: &RelativePlace) -> Option<Region> {
        self.start
            .to_address(origin)
            .map(|x| Region::new(x, self.length))
    }
}

impl Display for Section {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}->{}", self.start, self.length)
    }
}

#[derive(Clone, Debug, Default)]
struct RelativePlace {
    line: LineNumber,
    index: isize,
}

/// Signifies the location of a character within a view.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct Place {
    line: LineNumber,
    index: usize,
}

impl Place {
    fn to_address(&self, origin: &RelativePlace) -> Option<Address> {
        if self.line < origin.line {
            None
        } else {
            Some(Address::new(
                self.line.index() - origin.line.index(),
                (self.index as isize - origin.index) as usize,
            ))
        }
    }

    fn to_region(&self, origin: &RelativePlace) -> Option<Region> {
        self.to_address(origin)
            .map(|x| Region::new(x, Length::from(1)))
    }
}

impl Shr<usize> for Place {
    type Output = Place;

    fn shr(self, rhs: usize) -> Place {
        Place {
            index: self.index + rhs,
            ..self
        }
    }
}

impl Display for Place {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "ln {}, idx {}", self.line, self.index)
    }
}

/// Signifies a line number.
#[derive(Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, Debug)]
pub struct LineNumber(NonZeroUsize);

impl LineNumber {
    fn new(value: usize) -> Option<LineNumber> {
        NonZeroUsize::new(value).map(|x| LineNumber(x))
    }

    fn index(self) -> usize {
        self.0.get() - 1
    }
}

impl Display for LineNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", self.0)
    }
}

impl Default for LineNumber {
    fn default() -> LineNumber {
        // Safe to unwrap because 1 is a non-zero.
        LineNumber(NonZeroUsize::new(1).unwrap())
    }
}

impl Add<usize> for LineNumber {
    type Output = LineNumber;

    fn add(self, other: usize) -> LineNumber {
        // Safe to unwrap because self.0.get() > 0 and other >= 0.
        LineNumber::new(self.0.get() + other).unwrap()
    }
}

impl Add<isize> for LineNumber {
    type Output = LineNumber;

    fn add(self, other: isize) -> LineNumber {
        if other < 0 {
            self - (-other) as usize
        } else {
            self + other as usize
        }
    }
}

impl Sub<usize> for LineNumber {
    type Output = LineNumber;

    fn sub(self, other: usize) -> LineNumber {
        if self.0.get() > other {
            // Safe to unwrap because self.0.get() > other.
            LineNumber::new(self.0.get() - other).unwrap()
        } else {
            Default::default()
        }
    }
}

impl SubAssign<usize> for LineNumber {
    fn sub_assign(&mut self, other: usize) {
        *self = *self - other
    }
}

impl PartialEq<usize> for LineNumber {
    fn eq(&self, other: &usize) -> bool {
        self.0.get() == *other
    }
}

impl PartialOrd<usize> for LineNumber {
    fn partial_cmp(&self, other: &usize) -> Option<std::cmp::Ordering> {
        Some(self.0.get().cmp(other))
    }
}

trait Filter: Debug {
    fn id(&self) -> char;
    fn extract(&self, feature: &str, sections: &mut Vec<Section>, view: &View);
}

#[derive(Debug)]
struct LineFilter {
    pattern: Pattern,
}

impl Default for LineFilter {
    fn default() -> LineFilter {
        LineFilter {
            pattern: Pattern::define(
                "#" + (ChCls::Digit.rpt(SOME).name("line") + ChCls::End
                    | ChCls::Digit.rpt(SOME).name("start")
                        + "."
                        + ChCls::Digit.rpt(SOME).name("end")
                    | ChCls::Digit.rpt(SOME).name("origin")
                        + (("+".to_rec() | "-") + ChCls::Digit.rpt(SOME)).name("movement")),
            ),
        }
    }
}

impl Filter for LineFilter {
    fn id(&self) -> char {
        '#'
    }

    fn extract(&self, feature: &str, sections: &mut Vec<Section>, _view: &View) {
        let tokens = self.pattern.tokenize(feature);

        if let Some(Ok(line)) = tokens.get("line").map(|x| x.parse::<usize>()) {
            sections.retain(|&x| x.start.line == line);
        } else if let (Some(line_start), Some(line_end)) = (tokens.get("start"), tokens.get("end"))
        {
            if let (Ok(start), Ok(end)) = (line_start.parse::<usize>(), line_end.parse::<usize>()) {
                let top = cmp::min(start, end);
                let bottom = cmp::max(start, end);

                sections.retain(|&x| {
                    let row = x.start.line;
                    row >= top && row <= bottom
                })
            }
        } else if let (Some(line_origin), Some(line_movement)) =
            (tokens.get("origin"), tokens.get("movement"))
        {
            if let (Ok(origin), Ok(movement)) =
                (line_origin.parse::<usize>(), line_movement.parse::<isize>())
            {
                let end = (origin as isize + movement) as usize;
                let top = cmp::min(origin, end);
                let bottom = cmp::max(origin, end);

                sections.retain(|&x| {
                    let row = x.start.line;
                    row >= top && row <= bottom
                })
            }
        }
    }
}

#[derive(Debug)]
struct PatternFilter {
    pattern: Pattern,
}

impl Default for PatternFilter {
    fn default() -> PatternFilter {
        PatternFilter {
            pattern: Pattern::define("/" + ChCls::Any.rpt(SOME).name("pattern")),
        }
    }
}

impl Filter for PatternFilter {
    fn id(&self) -> char {
        '/'
    }

    fn extract(&self, feature: &str, sections: &mut Vec<Section>, view: &View) {
        if let Some(user_pattern) = self.pattern.tokenize(feature).get("pattern") {
            if let Ok(search_pattern) = Pattern::load(user_pattern.to_rec()) {
                let target_sections = sections.clone();
                sections.clear();

                for target_section in target_sections {
                    let target = view
                        .line(target_section.start.line)
                        .unwrap()
                        .chars()
                        .skip(target_section.start.index)
                        .collect::<String>();

                    for location in search_pattern.locate_iter(&target) {
                        sections.push(Section {
                            start: target_section.start >> location.start(),
                            length: Length::from(location.length()),
                        });
                    }
                }
            }
        }
    }
}
