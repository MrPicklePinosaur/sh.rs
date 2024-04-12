use std::collections::HashMap;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use shrs_utils::cursor_buffer::Location;

use super::line::LineState;

///Controls when snippet should be applied
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum ExpandSnippet {
    OnTab,
    OnSpace,
    #[default]
    Never,
    OnKey(KeyEvent),
}
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Position {
    #[default]
    Command,
    Anywhere,
}

pub struct SnippetInfo {
    ///value to be inserted
    pub value: String,
    ///where the snippet needs to be, to be expanded
    pub position: Position,
}
impl SnippetInfo {
    pub fn new<S: ToString>(value: S, position: Position) -> Self {
        Self {
            value: value.to_string(),
            position,
        }
    }
}
/// Snippets are substitutions that apply in the line when a trigger key is pressed
#[derive(Default)]
pub struct Snippets {
    pub snippets: HashMap<String, SnippetInfo>,
    pub expand_snippet: ExpandSnippet,
    pub enabled: bool,
}
impl Snippets {
    pub fn new(expand_snippet: ExpandSnippet) -> Self {
        Self {
            expand_snippet,
            snippets: HashMap::new(),
            enabled: true,
        }
    }
    pub fn add(&mut self, name: String, s: SnippetInfo) {
        self.snippets.insert(name, s);
    }
    /// Returns whether the event was matched or not.
    /// Always is mapped to when the user presses space
    pub fn should_expand(&self, event: &Event) -> bool {
        if !self.enabled {
            return false;
        }
        match self.expand_snippet {
            ExpandSnippet::OnSpace => {
                *event == Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            },
            ExpandSnippet::OnTab => {
                *event == Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            },

            ExpandSnippet::Never => false,
            ExpandSnippet::OnKey(k) => *event == Event::Key(k),
        }
    }
    pub fn get(&self, name: &String) -> Option<&SnippetInfo> {
        self.snippets.get(name)
    }
    pub fn enable(&mut self) {
        self.enabled = true
    }
    pub fn disable(&mut self) {
        self.enabled = false
    }
}
