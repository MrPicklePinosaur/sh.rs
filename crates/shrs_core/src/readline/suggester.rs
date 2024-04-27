use super::line::LineContents;
use crate::prelude::{History, States};

pub trait Suggester {
    fn suggest(&self, ctx: &States) -> Option<String>;
}
pub struct DefaultSuggester;
impl Suggester for DefaultSuggester {
    fn suggest(&self, ctx: &States) -> Option<String> {
        let res = ctx.get_mut::<LineContents>().get_full_command();
        if res.is_empty() {
            return None;
        }

        for s in ctx.get_mut::<Box<dyn History>>().iter() {
            if s.starts_with(&res) {
                return Some(s.to_owned());
            }
        }
        None
    }
}
