//! General purpose selection menu for shell

use std::{fmt::Display, io::Write};

use crossterm::{
    cursor::{MoveDown, MoveToColumn, MoveUp},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    QueueableCommand,
};

use crate::completion::Completion;

pub type Out = std::io::BufWriter<std::io::Stdout>;

pub trait Menu {
    type MenuItem;
    type PreviewItem: Display;

    fn next(&mut self);
    fn previous(&mut self);
    fn accept(&mut self) -> Option<&Self::MenuItem>;
    fn current_selection(&self) -> Option<&Self::MenuItem>;
    fn cursor(&self) -> u32;
    fn is_active(&self) -> bool;
    fn activate(&mut self);
    fn disactivate(&mut self);
    fn items(&self) -> Vec<&(Self::PreviewItem, Self::MenuItem)>;
    fn set_items(&mut self, items: Vec<(Self::PreviewItem, Self::MenuItem)>);

    fn selected_style(&self, out: &mut Out) -> crossterm::Result<()>;
    fn unselected_style(&self, out: &mut Out) -> crossterm::Result<()>;

    fn render(&self, out: &mut Out) -> anyhow::Result<()>;
    fn required_lines(&self) -> usize;
}

/// Simple menu that prompts user for a selection
pub struct DefaultMenu {
    selections: Vec<(String, Completion)>,
    /// Currently selected item
    cursor: u32,
    active: bool,
    max_columns: usize,
    max_rows: usize,
    column_padding: usize,
}

impl DefaultMenu {
    pub fn new() -> Self {
        DefaultMenu {
            selections: vec![],
            cursor: 0,
            active: false,
            max_columns: 2,
            max_rows: 5,
            column_padding: 2,
        }
    }
}

impl Menu for DefaultMenu {
    type MenuItem = Completion;
    type PreviewItem = String;

    fn next(&mut self) {
        if self.cursor as usize == self.selections.len().saturating_sub(1) {
            self.cursor = 0;
        } else {
            self.cursor += 1;
        }
    }
    fn previous(&mut self) {
        if self.cursor == 0 {
            self.cursor = self.selections.len().saturating_sub(1) as u32;
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }
    fn accept(&mut self) -> Option<&Self::MenuItem> {
        self.disactivate();
        self.current_selection()
    }
    fn current_selection(&self) -> Option<&Self::MenuItem> {
        self.selections.get(self.cursor as usize).map(|x| &x.1)
    }
    fn cursor(&self) -> u32 {
        self.cursor
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn activate(&mut self) {
        // dont activate if menu is empty
        self.active = !self.selections.is_empty();
    }
    fn disactivate(&mut self) {
        self.active = false;
    }
    fn items(&self) -> Vec<&(Self::PreviewItem, Self::MenuItem)> {
        // TODO is this the right way to case Vec<String> to Vec<&String> ??
        self.selections.iter().collect()
    }
    fn set_items(&mut self, mut items: Vec<(Self::PreviewItem, Self::MenuItem)>) {
        self.selections.clear();
        self.selections.append(&mut items);
        self.cursor = 0;
    }

    fn selected_style(&self, out: &mut Out) -> crossterm::Result<()> {
        execute!(
            out,
            SetBackgroundColor(Color::White),
            SetForegroundColor(Color::Black),
        )?;
        Ok(())
    }

    fn unselected_style(&self, out: &mut Out) -> crossterm::Result<()> {
        execute!(out, ResetColor)?;
        Ok(())
    }

    fn render(&self, out: &mut Out) -> anyhow::Result<()> {
        let mut i = 0;
        let mut column_start: usize = 0;

        self.unselected_style(out)?;
        for column in self.items().chunks(self.max_rows) {
            // length of the longest word in column
            let mut longest_word = 0;

            for menu_item in column.iter() {
                longest_word = longest_word.max(menu_item.0.len());
                out.queue(MoveDown(1))?;
                out.queue(MoveToColumn(column_start as u16))?;
                if self.cursor() as usize == i {
                    self.selected_style(out)?;
                }

                out.queue(Print(&menu_item.0))?;
                self.unselected_style(out)?;

                i += 1;
            }
            column_start += longest_word + self.column_padding;

            // move back up
            out.queue(MoveUp(column.len() as u16))?;
        }

        Ok(())
    }

    fn required_lines(&self) -> usize {
        self.items().len().min(self.max_rows) + 1
    }
}
