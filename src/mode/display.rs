//! Implements functionality for the application while in display mode.
use super::{Initiation, Operation, Output, Pane};
use crate::{file::Explorer, ptr::Mrc};
use std::cell::Ref;

/// The [`Processor`] of the display mode.
#[derive(Clone, Debug)]
pub(crate) struct Processor {
    /// The [`Explorer`] of the application.
    explorer: Mrc<dyn Explorer>,
    /// The [`Pane`] of the application.
    pane: Mrc<Pane>,
}

impl Processor {
    /// Creates a new `Processor`.
    pub(crate) fn new(pane: &Mrc<Pane>, explorer: &Mrc<dyn Explorer>) -> Self {
        Self {
            explorer: Mrc::clone(explorer),
            pane: Mrc::clone(pane),
        }
    }
}

impl super::Processor for Processor {
    fn enter(&mut self, initiation: &Option<Initiation>) -> Output<()> {
        let mut pane = self.pane.borrow_mut();

        match initiation {
            Some(Initiation::SetView(path)) => {
                pane.change(&self.explorer, path)?;
            }
            Some(Initiation::Save) => {
                let explorer: Ref<'_, (dyn Explorer)> = self.explorer.borrow();
                explorer.write(&pane.path, &pane.data)?;
            }
            _ => (),
        }

        pane.wipe();

        Ok(())
    }

    fn decode(&mut self, input: char) -> Output<Operation> {
        let mut pane = self.pane.borrow_mut();

        match input {
            '.' => Ok(Operation::enter_command()),
            '#' | '/' => Ok(Operation::enter_filter(input)),
            'j' => {
                pane.scroll_down();
                Ok(Operation::maintain())
            }
            'k' => {
                pane.scroll_up();
                Ok(Operation::maintain())
            }
            _ => Ok(Operation::maintain()),
        }
    }
}
