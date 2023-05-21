use std::{
    collections::HashMap,
    fs::File,
    io::{stdout, BufRead, BufWriter, Write},
    path::Path,
    process::{Child, Stdio},
    time::Instant,
};

use lazy_static::lazy_static;
use shrs_core::{
    builtin::Builtins,
    command_output, dummy_child,
    hooks::{BeforeCommandCtx, Hooks, JobExitCtx, StartupCtx},
    run_external_command, sig_handler, Alias, Context, Env, ExitStatus, Jobs, Runtime, Shell,
    State, Theme,
};
use shrs_lang::{ast, Lexer, Parser, RESERVED_WORDS};
use shrs_line::{DefaultPrompt, Line, Prompt};
use thiserror::Error;

use crate::plugin::Plugin;

/// Unified shell config struct
#[derive(Builder)]
#[builder(pattern = "owned")]
#[builder(setter(prefix = "with"))]
pub struct ShellConfig {
    #[builder(default = "Hooks::default()")]
    pub hooks: Hooks,

    #[builder(default = "Builtins::default()")]
    pub builtins: Builtins,

    #[builder(default = "Line::default()")]
    pub readline: Line,

    #[builder(default = "Alias::new()")]
    pub alias: Alias,

    /// Environment variables
    #[builder(default = "Env::new()")]
    pub env: Env,

    /// List of defined functions
    #[builder(default = "HashMap::new()")]
    pub functions: HashMap<String, Box<ast::Command>>,

    /// Color theme
    #[builder(default = "Theme::default()")]
    pub theme: Theme,

    /// Plugins
    #[builder(default = "Vec::new()")]
    #[builder(setter(custom))]
    pub plugins: Vec<Box<dyn Plugin>>,

    /// Globally accessable state
    #[builder(default = "State::new()")]
    #[builder(setter(custom))]
    pub state: State,
}

impl ShellConfigBuilder {
    pub fn with_plugin(mut self, plugin: impl Plugin + 'static) -> Self {
        let mut cur_plugin = self.plugins.unwrap_or(vec![]);
        cur_plugin.push(Box::new(plugin));
        self.plugins = Some(cur_plugin);
        self
    }
    pub fn with_state<T: 'static>(mut self, state: T) -> Self {
        let mut cur_state = self.state.unwrap_or(State::new());
        cur_state.insert(state);
        self.state = Some(cur_state);
        self
    }
}

impl ShellConfig {
    pub fn run(mut self) -> anyhow::Result<()> {
        // TODO some default values for Context and Runtime are duplicated by the #[builder(default = "...")]
        // calls in ShellConfigBuilder, so we are sort of defining the full default here. Maybe end
        // up implementing Default for Context and Runtime

        // run plugins first
        let plugins = self.plugins.drain(..).collect::<Vec<_>>();
        for plugin in plugins {
            plugin.init(&mut self);
        }

        let mut ctx = Context {
            alias: self.alias,
            out: BufWriter::new(stdout()),
            state: self.state,
            jobs: Jobs::new(),
            startup_time: Instant::now(),
        };
        let mut rt = Runtime {
            env: self.env,
            working_dir: std::env::current_dir().unwrap(),
            // TODO currently hardcoded
            name: "shrs".into(),
            // TODO currently unused (since we have not implemented functions etc)
            args: vec![],
            exit_status: 0,
            functions: self.functions,
        };
        let sh = Shell {
            builtins: self.builtins,
            hooks: self.hooks,
            theme: self.theme,
        };
        let mut readline = self.readline;

        run_shell(&sh, &mut ctx, &mut rt, &mut readline)
    }
}

#[derive(Error, Debug)]
pub enum Error {
    /// Error when attempting file redirection
    #[error("Redirection Error: {0}")]
    Redirect(std::io::Error),
    /// Error emitted by hook
    #[error("Hook Error:")]
    Hook(),
}

fn run_shell(
    sh: &Shell,
    ctx: &mut Context,
    rt: &mut Runtime,
    readline: &mut Line,
) -> anyhow::Result<()> {
    // init stuff
    sig_handler()?;
    rt.env.load();
    let mut parser = Parser::new();

    let res = sh.hooks.startup.run(
        sh,
        ctx,
        rt,
        &StartupCtx {
            startup_time: ctx.startup_time.elapsed(),
        },
    );

    if let Err(_e) = res {
        // TODO log that startup hook failed
    }

    loop {
        let line = readline.read_line(sh, ctx, rt);

        // attempt to expand alias
        // TODO IFS
        let mut words = line
            .split(' ')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if let Some(first) = words.get_mut(0) {
            if let Some(expanded) = ctx.alias.get(first.clone()) {
                *first = expanded;
            }
        }
        let line = words.join(" ");

        // TODO not sure if hook should run here (since not all vars are expanded yet)
        let hook_ctx = BeforeCommandCtx {
            raw_command: line.clone(),
            command: line.clone(),
        };
        sh.hooks.before_command.run(sh, ctx, rt, &hook_ctx)?;

        // TODO rewrite the error handling here better
        let lexer = Lexer::new(&line);
        let cmd = match parser.parse(lexer) {
            Ok(cmd) => cmd,
            Err(e) => {
                // TODO detailed parse errors
                eprintln!("{e}");
                continue;
            },
        };
        let mut cmd_handle =
            match eval_command(sh, ctx, rt, &cmd, Stdio::inherit(), Stdio::inherit(), None) {
                Ok(cmd_handle) => cmd_handle,
                Err(e) => {
                    eprintln!("{e}");
                    continue;
                },
            };
        command_output(sh, ctx, rt, &mut cmd_handle)?;

        // check up on running jobs
        let mut exit_statuses = vec![];
        ctx.jobs.retain(|status: ExitStatus| {
            exit_statuses.push(status);
        });

        for status in exit_statuses.into_iter() {
            sh.hooks.job_exit.run(sh, ctx, rt, &JobExitCtx { status });
        }
    }
}

// TODO function signature is very ugly
// TODO maybe make this a method of Command
fn eval_command(
    sh: &Shell,
    ctx: &mut Context,
    rt: &mut Runtime,
    cmd: &ast::Command,
    stdin: Stdio,
    stdout: Stdio,
    _pgid: Option<i32>,
) -> anyhow::Result<Child> {
    match cmd {
        ast::Command::Simple {
            assigns,
            args,
            redirects,
        } => {
            let mut it = args.iter();

            // Retrieve command name or return immediately (empty command)
            let cmd_name = match it.next() {
                Some(cmd_name) => cmd_name,
                None => return dummy_child(),
            };
            let args = it.map(|a| (*a).clone()).collect::<Vec<_>>();

            // println!("redirects {:?}", redirects);
            // println!("assigns {:?}", assigns);

            // file redirections
            // TODO: current behavior, only one read and write operation is allowed, the latter ones will override the behavior of eariler ones
            let mut cur_stdin = stdin;
            let mut cur_stdout = stdout;
            for redirect in redirects {
                let filename = Path::new(&*redirect.file);
                // TODO not making use of file descriptor at all right now
                let _n = match &redirect.n {
                    Some(n) => *n,
                    None => 1,
                };
                match redirect.mode {
                    ast::RedirectMode::Read => {
                        let file_handle = File::options()
                            .read(true)
                            .open(filename)
                            .map_err(Error::Redirect)?;
                        cur_stdin = Stdio::from(file_handle);
                    },
                    ast::RedirectMode::Write => {
                        let file_handle = File::options()
                            .write(true)
                            .create_new(true)
                            .open(filename)
                            .map_err(Error::Redirect)?;
                        cur_stdout = Stdio::from(file_handle);
                    },
                    ast::RedirectMode::ReadAppend => {
                        let file_handle = File::options()
                            .read(true)
                            .append(true)
                            .open(filename)
                            .map_err(Error::Redirect)?;
                        cur_stdin = Stdio::from(file_handle);
                    },
                    ast::RedirectMode::WriteAppend => {
                        let file_handle = File::options()
                            .write(true)
                            .append(true)
                            .create_new(true)
                            .open(filename)
                            .map_err(Error::Redirect)?;
                        cur_stdout = Stdio::from(file_handle);
                    },
                    ast::RedirectMode::ReadDup => {
                        unimplemented!()
                    },
                    ast::RedirectMode::WriteDup => {
                        unimplemented!()
                    },
                    ast::RedirectMode::ReadWrite => {
                        let file_handle = File::options()
                            .read(true)
                            .write(true)
                            .create_new(true)
                            .open(filename)
                            .map_err(Error::Redirect)?;
                        cur_stdin = Stdio::from(file_handle.try_clone().unwrap());
                        cur_stdout = Stdio::from(file_handle);
                    },
                };
            }

            // TODO which stdin var to use?, previous command or from file redirection?

            // TODO doing args subst here is a waste if we evaluating function body
            let subst_args = args.iter().map(|x| envsubst(rt, x)).collect::<Vec<_>>();

            for (builtin_name, builtin_cmd) in sh.builtins.iter() {
                if builtin_name == &cmd_name.as_str() {
                    return builtin_cmd.run(sh, ctx, rt, &subst_args);
                }
            }

            // otherwise look for defined functions
            let cmd_body = rt.functions.get(cmd_name.as_str()).cloned();
            match cmd_body {
                Some(ref cmd_body) => eval_command(
                    sh,
                    ctx,
                    rt,
                    cmd_body,
                    Stdio::inherit(),
                    Stdio::piped(),
                    None,
                ),
                None => run_external_command(
                    sh,
                    ctx,
                    rt,
                    cmd_name,
                    &subst_args,
                    cur_stdin,
                    cur_stdout,
                    None,
                    assigns,
                ),
            }
        },
        ast::Command::Pipeline(a_cmd, b_cmd) => {
            // TODO double check that pgid works properly for pipelines that are longer than one pipe (left recursiveness of parser might mess this up)
            let mut a_cmd_handle = eval_command(sh, ctx, rt, a_cmd, stdin, Stdio::piped(), None)?;
            let piped_stdin = Stdio::from(a_cmd_handle.stdout.take().unwrap());
            let pgid = a_cmd_handle.id();
            let b_cmd_handle =
                eval_command(sh, ctx, rt, b_cmd, piped_stdin, stdout, Some(pgid as i32))?;
            Ok(b_cmd_handle)
        },
        ast::Command::Or(a_cmd, b_cmd) | ast::Command::And(a_cmd, b_cmd) => {
            let negate = match cmd {
                ast::Command::Or { .. } => false,
                ast::Command::And { .. } => true,
                _ => unreachable!(),
            };
            // TODO double check if these stdin and stdou params are correct
            let mut a_cmd_handle =
                eval_command(sh, ctx, rt, a_cmd, Stdio::inherit(), Stdio::piped(), None)?;
            let output_status = command_output(sh, ctx, rt, &mut a_cmd_handle)?;
            if output_status.success() ^ negate {
                // TODO return something better (indicate that command failed with exit code)
                return dummy_child();
            }
            let b_cmd_handle =
                eval_command(sh, ctx, rt, b_cmd, Stdio::inherit(), Stdio::piped(), None)?;
            Ok(b_cmd_handle)
        },
        ast::Command::Not(cmd) => {
            // TODO exit status negate
            let cmd_handle = eval_command(sh, ctx, rt, cmd, stdin, stdout, None)?;
            Ok(cmd_handle)
        },
        ast::Command::AsyncList(a_cmd, b_cmd) => {
            let a_cmd_handle =
                eval_command(sh, ctx, rt, a_cmd, Stdio::inherit(), Stdio::piped(), None)?;

            match b_cmd {
                None => {
                    // TODO might need a Command display trait implementation
                    ctx.jobs.push(a_cmd_handle, String::new());
                    dummy_child()
                },
                Some(b_cmd) => {
                    let b_cmd_handle =
                        eval_command(sh, ctx, rt, b_cmd, Stdio::inherit(), Stdio::piped(), None)?;
                    Ok(b_cmd_handle)
                },
            }
        },
        ast::Command::SeqList(a_cmd, b_cmd) => {
            // TODO very similar to AsyncList
            let mut a_cmd_handle =
                eval_command(sh, ctx, rt, a_cmd, Stdio::inherit(), Stdio::piped(), None)?;

            match b_cmd {
                None => Ok(a_cmd_handle),
                Some(b_cmd) => {
                    command_output(sh, ctx, rt, &mut a_cmd_handle)?;
                    let b_cmd_handle =
                        eval_command(sh, ctx, rt, b_cmd, Stdio::inherit(), Stdio::piped(), None)?;
                    Ok(b_cmd_handle)
                },
            }
        },
        ast::Command::Subshell(cmd) => {
            // TODO rn history is being copied too, history (and also alias?) really should be global
            // maybe seperate out global context and runtime context into two structs?
            let mut new_rt = rt.clone();
            let cmd_handle = eval_command(
                sh,
                ctx,
                &mut new_rt,
                cmd,
                Stdio::inherit(),
                Stdio::piped(),
                None,
            )?;
            Ok(cmd_handle)
        },
        ast::Command::If { conds, else_part } => {
            // TODO throw proper error here
            assert!(!conds.is_empty());

            for ast::Condition { cond, body } in conds {
                let mut cond_handle =
                    eval_command(sh, ctx, rt, cond, Stdio::inherit(), Stdio::piped(), None)?;
                // TODO sorta similar to and statements
                let output_status = command_output(sh, ctx, rt, &mut cond_handle)?;
                if output_status.success() {
                    let body_handle =
                        eval_command(sh, ctx, rt, body, Stdio::inherit(), Stdio::piped(), None)?;
                    return Ok(body_handle);
                }
            }

            if let Some(else_part) = else_part {
                let else_handle = eval_command(
                    sh,
                    ctx,
                    rt,
                    else_part,
                    Stdio::inherit(),
                    Stdio::piped(),
                    None,
                )?;
                return Ok(else_handle);
            }

            dummy_child()
        },
        ast::Command::While { cond, body } | ast::Command::Until { cond, body } => {
            let negate = match cmd {
                ast::Command::While { .. } => false,
                ast::Command::Until { .. } => true,
                _ => unreachable!(),
            };

            loop {
                let mut cond_handle =
                    eval_command(sh, ctx, rt, cond, Stdio::inherit(), Stdio::piped(), None)?;
                // TODO sorta similar to if statements
                let output_status = command_output(sh, ctx, rt, &mut cond_handle)?;
                if output_status.success() ^ negate {
                    let mut body_handle =
                        eval_command(sh, ctx, rt, body, Stdio::inherit(), Stdio::piped(), None)?;
                    command_output(sh, ctx, rt, &mut body_handle)?;
                } else {
                    break; // TODO not sure if there should be break here
                }
            }
            dummy_child()
        },
        ast::Command::For {
            name,
            wordlist,
            body,
        } => {
            // expand wordlist
            let mut expanded = vec![];
            for word in wordlist {
                // TODO use IFS variable for this
                for subword in word.split(' ') {
                    expanded.push(subword);
                }
            }

            // execute body
            for word in expanded {
                // TODO should have seperate variable struct instead of env
                rt.env.set(name, word); // TODO unset the var after the loop?
                let mut body_handle =
                    eval_command(sh, ctx, rt, body, Stdio::inherit(), Stdio::piped(), None)?;
                command_output(sh, ctx, rt, &mut body_handle)?;
            }

            dummy_child()
        },
        ast::Command::Case { word, arms } => {
            // println!("word {:?}, arms {:?}", word, arms);

            let subst_word = envsubst(rt, word);

            for ast::CaseArm { pattern, body } in arms {
                if pattern.iter().any(|x| x == &subst_word) {
                    let mut body_handle =
                        eval_command(sh, ctx, rt, body, Stdio::inherit(), Stdio::piped(), None)?;
                    command_output(sh, ctx, rt, &mut body_handle)?;
                    // TODO should we break? (should multiple match arms be matched?)
                }
            }

            dummy_child()
        },
        ast::Command::Fn { fname, body } => {
            if RESERVED_WORDS.contains(&fname.as_str()) {
                eprintln!("function nane cannot be a reserved keyword");
                return dummy_child(); // TODO come up with better return value
            }

            // TODO hook for redefining function?
            rt.functions.insert(fname.to_string(), body.to_owned());

            dummy_child()
        },
        ast::Command::None => dummy_child(),
    }
}

/// Performs environment substation on a string
// TODO regex replace might not be the best way. could also recognize the env var during parsing
// TODO handle escaped characters
fn envsubst(rt: &mut Runtime, arg: &str) -> String {
    use regex::Regex;

    lazy_static! {
        static ref R_0: Regex = Regex::new(r"\$(?P<env>[a-zA-Z_]+)").unwrap(); // no braces
        static ref R_1: Regex = Regex::new(r"\$\{(?P<env>[a-zA-Z_]+)\}").unwrap(); // with braces
        static ref R_2: Regex = Regex::new(r"~").unwrap(); // tilde
    }

    let mut subst = arg.to_string();

    // substitute special parameters first
    subst = subst.as_str().replace("$?", &rt.exit_status.to_string());
    subst = subst.as_str().replace("$#", &rt.args.len().to_string());
    subst = subst.as_str().replace("$0", &rt.name);

    for cap in R_0.captures_iter(arg) {
        // look up env var
        let var = &cap["env"];
        // TODO stupid code
        let val = match rt.env.get(var) {
            Some(val) => val.clone(),
            None => String::new(),
        };
        let fmt_env = format!("${var}"); // format $VAR
        subst = subst.as_str().replace(&fmt_env, &val);
    }

    // TODO this is dumb stupid and bad repeated code
    for cap in R_1.captures_iter(arg) {
        let var = &cap["env"];
        let val = match rt.env.get(var) {
            Some(val) => val.clone(),
            None => String::new(),
        };
        let fmt_env = format!("${{{var}}}"); // format ${VAR}
        subst = subst.as_str().replace(&fmt_env, &val);
    }

    // tilde substitution
    let home = match rt.env.get("HOME") {
        Some(home) => home.as_str(),
        None => "",
    };
    let subst = R_2.replace_all(&subst, home).to_string();

    subst
}

/*
#[cfg(test)]
mod tests {
    use super::{envsubst, Runtime};

    // #[test]
    // fn envsubst_test() {
    //     let mut rt = Runtime::default();
    //     rt.env.set("EDITOR", "vim");
    //     rt.env.set("SHELL", "/bin/shrs");
    //     let text = "$SHELL ${EDITOR}";
    //     let subst = envsubst(&mut rt, text);
    //     assert_eq!(subst, String::from("/bin/shrs vim"));
    // }

    // #[test]
    // fn path_execs_test() {
    //     println!("{:?}", find_executables_in_path("/usr/bin:/usr/local/bin"));
    // }
}
*/
