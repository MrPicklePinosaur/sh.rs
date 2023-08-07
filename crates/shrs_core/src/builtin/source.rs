use std::{
    env,
    fs::read_to_string,
    path::{Path, PathBuf},
    process::Command,
};

use lazy_static::lazy_static;
use regex::Regex;

use super::{BuiltinCmd, BuiltinStatus};
use crate::shell::{Context, Runtime, Shell};

lazy_static! {
    static ref SHEBANG_REGEX: Regex = Regex::new(r"#!(?P<interp>.+)").unwrap();
}

#[derive(Default)]
pub struct SourceBuiltin {}

impl BuiltinCmd for SourceBuiltin {
    fn run(
        &self,
        sh: &Shell,
        ctx: &mut Context,
        rt: &mut Runtime,
        args: &Vec<String>,
    ) -> anyhow::Result<BuiltinStatus> {
        if args.len() != 1 {
            return Ok(BuiltinStatus::error());
        }

        let file_path_str = args.get(0).unwrap();
        let file_path = PathBuf::from(file_path_str);
        let file_contents = read_to_string(file_path)?;

        // read shebang from first line
        let mut it = file_contents.lines();

        let interp = it
            .next()
            .and_then(|first_line| SHEBANG_REGEX.captures(first_line))
            .and_then(|capture| capture.name("interp"));

        match interp {
            Some(interp) => {
                println!("using interp {} at {}", interp.as_str(), file_path_str);
                let mut child = Command::new(interp.as_str())
                    .args(vec![file_path_str])
                    .spawn()?;

                // need command output here
                // TODO temp disable this
                // command_output(sh, ctx, rt, &mut child)?;

                Ok(BuiltinStatus::success())
            },
            None => {
                // otherwise evaluate with self

                todo!()
            },
        }
    }
}
