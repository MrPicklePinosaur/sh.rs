//! Builtin commands
//!
//! The main difference between builtin commands and external commands is that builtin commands
//! have access to the shell's context during execution. This may be useful if you specifically
//! need to query or mutate the shell's state. Some uses of this include switching the working
//! directory, calling hooks or accessing the state store.

mod alias;
mod cd;
mod debug;
mod exit;
mod export;
mod help;
mod history;
mod jobs;
mod source;
mod r#type;
mod unalias;

use std::collections::{hash_map::Iter, HashMap};

pub use self::{
    alias::AliasBuiltin, cd::CdBuiltin, debug::DebugBuiltin, exit::ExitBuiltin,
    export::ExportBuiltin, help::HelpBuiltin, history::HistoryBuiltin, jobs::JobsBuiltin,
    r#type::TypeBuiltin, source::SourceBuiltin, unalias::UnaliasBuiltin,
};
use crate::{
    prelude::{CmdOutput, Ctx, Hook, StartupCtx, States},
    shell::{Runtime, Shell},
};

// TODO could prob just be a map, to support arbitrary (user defined even) number of builtin commands
// just provide an easy way to override the default ones
/// Store for all registered builtin commands
pub struct Builtins {
    builtins: HashMap<&'static str, Box<dyn BuiltinCmd>>,
}

// TODO a lot of this api is silly, perhaps just expose the entire hashmap
impl Builtins {
    /// Initializes a builtin container with no registered builtins
    ///
    /// You probably want to use `Builtins::default()` instead to get some sensible default
    /// builtins to use, then override the ones you want
    pub fn new() -> Self {
        Builtins {
            builtins: HashMap::new(),
        }
    }

    /// Insert a builtin command of the given name
    ///
    /// If a builtin of the same name has been registered, it will be overwritten.
    pub fn insert(&mut self, name: &'static str, builtin: impl BuiltinCmd + 'static) {
        self.builtins.insert(name, Box::new(builtin));
    }

    /// Get iterator of all registered builtin commands
    pub fn iter(&self) -> Iter<'_, &str, Box<dyn BuiltinCmd>> {
        self.builtins.iter()
    }

    /// Find a builtin by name
    // Clippy thinks this shouldn't be a box, but it does not compile if you follow the warning
    #[allow(clippy::borrowed_box)]
    pub fn get(&self, name: &'static str) -> Option<&Box<dyn BuiltinCmd>> {
        self.builtins.get(name)
    }
}

impl Default for Builtins {
    fn default() -> Self {
        Builtins {
            builtins: HashMap::from([
                (
                    "history",
                    Box::<HistoryBuiltin>::default() as Box<dyn BuiltinCmd>,
                ),
                ("exit", Box::<ExitBuiltin>::default() as Box<dyn BuiltinCmd>),
                ("cd", Box::<CdBuiltin>::default() as Box<dyn BuiltinCmd>),
                (
                    "debug",
                    Box::<DebugBuiltin>::default() as Box<dyn BuiltinCmd>,
                ),
                (
                    "export",
                    Box::<ExportBuiltin>::default() as Box<dyn BuiltinCmd>,
                ),
                (
                    "alias",
                    Box::<AliasBuiltin>::default() as Box<dyn BuiltinCmd>,
                ),
                (
                    "unalias",
                    Box::<UnaliasBuiltin>::default() as Box<dyn BuiltinCmd>,
                ),
                (
                    "source",
                    Box::<SourceBuiltin>::default() as Box<dyn BuiltinCmd>,
                ),
                (
                    "type",
                    Box::new(TypeBuiltin::default()) as Box<dyn BuiltinCmd>,
                ),
                ("jobs", Box::<JobsBuiltin>::default() as Box<dyn BuiltinCmd>),
                ("help", Box::<HelpBuiltin>::default() as Box<dyn BuiltinCmd>),
            ]),
        }
    }
}

/// Implement this trait to define your own builtin command
pub trait BuiltinCmd {
    fn run(&self, sh: &Shell, states: &mut States, args: &[String]) -> anyhow::Result<CmdOutput>;
}
