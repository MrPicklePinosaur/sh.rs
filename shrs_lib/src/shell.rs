use std::{
    env,
    fs::File,
    io::{stdin, stdout, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Output, Stdio},
};

use anyhow::anyhow;
use reedline::{History, HistoryItem};

use crate::{
    alias::Alias,
    ast::{self, Assign},
    builtin::Builtins,
    env::Env,
    history::MemHistory,
    lexer::Lexer,
    parser,
    prompt::CustomPrompt,
    signal::sig_handler,
};

pub fn simple_error() {}

/// Default formmater for displaying the exit code of the previous command
pub fn simple_exit_code(code: i32) {
    println!("[exit +{}]", code);
}

pub type ErrorCommand = fn();
pub type ExitCodeCommand = fn(i32);
// TODO ideas for hooks:
//   - exit hook: print message when shell exits (by exit builtin)

pub struct Hooks {
    /// User defined command for formatting shell error messages
    pub error_command: ErrorCommand,
    /// User defined command for formatting exit code of previous command
    pub exit_code_command: ExitCodeCommand,
}

impl Default for Hooks {
    fn default() -> Self {
        Hooks {
            error_command: simple_error,
            exit_code_command: simple_exit_code,
        }
    }
}

#[derive(Default)]
pub struct Shell {
    pub hooks: Hooks,
    pub builtins: Builtins,
    pub prompt: CustomPrompt,
}

// (shared) shell context
pub struct Context {
    pub history: Box<dyn History>,
    pub alias: Alias,
}

impl Default for Context {
    fn default() -> Self {
        Context {
            history: Box::new(MemHistory::new()),
            alias: Alias::new(),
        }
    }
}

// Runtime context for the shell
#[derive(Clone)]
pub struct Runtime {
    pub working_dir: PathBuf,
    pub env: Env,
}

impl Default for Runtime {
    fn default() -> Self {
        Runtime {
            env: Env::new(),
            working_dir: std::env::current_dir().unwrap(),
        }
    }
}

impl Shell {
    pub fn run(&self, ctx: &mut Context, rt: &mut Runtime) -> anyhow::Result<()> {
        use reedline::{
            default_vi_insert_keybindings, default_vi_normal_keybindings, Reedline, Signal, Vi,
        };

        // init stuff
        sig_handler()?;
        rt.env.load();

        let mut line_editor = Reedline::create().with_edit_mode(Box::new(Vi::new(
            default_vi_insert_keybindings(),
            default_vi_normal_keybindings(),
        )));

        loop {
            // (self.hooks.prompt_command)();

            let sig = line_editor.read_line(&self.prompt);
            let line = match sig {
                Ok(Signal::Success(buffer)) => buffer,
                x => {
                    println!("got event {:?}", x);
                    continue;
                },
            };

            // attempt to expand alias
            let expanded = ctx.alias.get(&line).unwrap_or(&line).clone();

            // TODO rewrite the error handling here better
            let lexer = Lexer::new(&expanded);
            let mut parser = parser::ParserContext::new();
            let cmd = match parser.parse(lexer) {
                Ok(cmd) => cmd,
                Err(e) => {
                    eprintln!("{}", e);
                    continue;
                },
            };
            let cmd_handle =
                match self.eval_command(ctx, rt, &cmd, Stdio::inherit(), Stdio::piped(), None) {
                    Ok(cmd_handle) => cmd_handle,
                    Err(e) => {
                        eprintln!("{}", e);
                        continue;
                    },
                };
            self.command_output(cmd_handle)?;
        }
    }

    // TODO function signature is very ugly
    // TODO maybe make this a method of Command
    pub fn eval_command(
        &self,
        ctx: &mut Context,
        rt: &mut Runtime,
        cmd: &ast::Command,
        stdin: Stdio,
        stdout: Stdio,
        pgid: Option<i32>,
    ) -> anyhow::Result<Child> {
        match cmd {
            ast::Command::Simple {
                assigns,
                args,
                redirects,
            } => {
                if args.len() == 0 {
                    return Err(anyhow!("command is empty"));
                }
                // println!("redirects {:?}", redirects);
                // println!("assigns {:?}", assigns);

                // file redirections
                // TODO: current behavior, only one read and write operation is allowed, the latter ones will override the behavior of eariler ones
                let mut cur_stdin = stdin;
                let mut cur_stdout = stdout;
                for redirect in redirects {
                    let filename = Path::new(&*redirect.file);
                    // TODO not making use of file descriptor at all right now
                    let n = match &redirect.n {
                        Some(n) => *n,
                        None => 1,
                    };
                    match redirect.mode {
                        ast::RedirectMode::Read => {
                            let file_handle = File::options().read(true).open(filename).unwrap();
                            cur_stdin = Stdio::from(file_handle);
                        },
                        ast::RedirectMode::Write => {
                            let file_handle = File::options()
                                .write(true)
                                .create_new(true)
                                .open(filename)
                                .unwrap();
                            cur_stdout = Stdio::from(file_handle);
                        },
                        ast::RedirectMode::ReadAppend => {
                            let file_handle = File::options()
                                .read(true)
                                .append(true)
                                .open(filename)
                                .unwrap();
                            cur_stdin = Stdio::from(file_handle);
                        },
                        ast::RedirectMode::WriteAppend => {
                            let file_handle = File::options()
                                .write(true)
                                .append(true)
                                .create_new(true)
                                .open(filename)
                                .unwrap();
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
                                .unwrap();
                            cur_stdin = Stdio::from(file_handle.try_clone().unwrap());
                            cur_stdout = Stdio::from(file_handle);
                        },
                    };
                }

                let mut it = args.into_iter();
                let cmd_name = &it.next().unwrap();
                let args = it
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|a| (*a).clone())
                    .collect();

                // TODO which stdin var to use?, previous command or from file redirection?

                // TODO currently don't support assignment for builtins (should it be supported even?)
                match cmd_name.as_str() {
                    "cd" => self.builtins.cd.run(ctx, rt, &args),
                    "exit" => self.builtins.exit.run(ctx, rt, &args),
                    "history" => self.builtins.history.run(ctx, rt, &args),
                    "debug" => self.builtins.debug.run(ctx, rt, &args),
                    _ => self.run_external_command(
                        ctx, rt, &cmd_name, &args, cur_stdin, cur_stdout, None, assigns,
                    ),
                }
            },
            ast::Command::Pipeline(a_cmd, b_cmd) => {
                // TODO double check that pgid works properly for pipelines that are longer than one pipe (left recursiveness of parser might mess this up)
                let mut a_cmd_handle =
                    self.eval_command(ctx, rt, a_cmd, stdin, Stdio::piped(), None)?;
                let piped_stdin = Stdio::from(a_cmd_handle.stdout.take().unwrap());
                let pgid = a_cmd_handle.id();
                let b_cmd_handle =
                    self.eval_command(ctx, rt, b_cmd, piped_stdin, stdout, Some(pgid as i32))?;
                Ok(b_cmd_handle)
            },
            ast::Command::Or(a_cmd, b_cmd) | ast::Command::And(a_cmd, b_cmd) => {
                let negate = match cmd {
                    ast::Command::Or { .. } => false,
                    ast::Command::And { .. } => true,
                    _ => unreachable!(),
                };
                // TODO double check if these stdin and stdou params are correct
                let a_cmd_handle =
                    self.eval_command(ctx, rt, a_cmd, Stdio::inherit(), Stdio::piped(), None)?;
                if let Some(output) = self.command_output(a_cmd_handle)? {
                    if output.status.success() ^ negate {
                        // TODO return something better (indicate that command failed with exit code)
                        return dummy_child();
                    }
                }
                let b_cmd_handle =
                    self.eval_command(ctx, rt, b_cmd, Stdio::inherit(), Stdio::piped(), None)?;
                Ok(b_cmd_handle)
            },
            ast::Command::Not(cmd) => {
                // TODO exit status negate
                let cmd_handle = self.eval_command(ctx, rt, cmd, stdin, stdout, None)?;
                Ok(cmd_handle)
            },
            ast::Command::AsyncList(a_cmd, b_cmd) => {
                let a_cmd_handle =
                    self.eval_command(ctx, rt, a_cmd, Stdio::inherit(), Stdio::piped(), None)?;

                match b_cmd {
                    None => Ok(a_cmd_handle),
                    Some(b_cmd) => {
                        let b_cmd_handle = self.eval_command(
                            ctx,
                            rt,
                            b_cmd,
                            Stdio::inherit(),
                            Stdio::piped(),
                            None,
                        )?;
                        Ok(b_cmd_handle)
                    },
                }
            },
            ast::Command::SeqList(a_cmd, b_cmd) => {
                // TODO very similar to AsyncList
                let a_cmd_handle =
                    self.eval_command(ctx, rt, a_cmd, Stdio::inherit(), Stdio::piped(), None)?;

                match b_cmd {
                    None => Ok(a_cmd_handle),
                    Some(b_cmd) => {
                        self.command_output(a_cmd_handle)?;
                        let b_cmd_handle = self.eval_command(
                            ctx,
                            rt,
                            b_cmd,
                            Stdio::inherit(),
                            Stdio::piped(),
                            None,
                        )?;
                        Ok(b_cmd_handle)
                    },
                }
            },
            ast::Command::Subshell(cmd) => {
                // TODO rn history is being copied too, history (and also alias?) really should be global
                // maybe seperate out global context and runtime context into two structs?
                let mut new_rt = rt.clone();
                let cmd_handle = self.eval_command(
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
                assert!(conds.len() >= 1);

                for ast::Condition { cond, body } in conds {
                    let cond_handle =
                        self.eval_command(ctx, rt, cond, Stdio::inherit(), Stdio::piped(), None)?;
                    // TODO sorta similar to and statements
                    if let Some(output) = self.command_output(cond_handle)? {
                        if output.status.success() {
                            let body_handle = self.eval_command(
                                ctx,
                                rt,
                                body,
                                Stdio::inherit(),
                                Stdio::piped(),
                                None,
                            )?;
                            return Ok(body_handle);
                        }
                    }
                }

                if let Some(else_part) = else_part {
                    let else_handle = self.eval_command(
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
                    let cond_handle =
                        self.eval_command(ctx, rt, cond, Stdio::inherit(), Stdio::piped(), None)?;
                    // TODO sorta similar to if statements
                    if let Some(output) = self.command_output(cond_handle)? {
                        if output.status.success() ^ negate {
                            let body_handle = self.eval_command(
                                ctx,
                                rt,
                                body,
                                Stdio::inherit(),
                                Stdio::piped(),
                                None,
                            )?;
                            self.command_output(body_handle)?;
                        } else {
                            break;
                        }
                    } else {
                        break; // TODO not sure if there should be break here
                    }
                }
                dummy_child()
            },
            ast::Command::None => dummy_child(),
        }
    }

    fn run_external_command(
        &self,
        ctx: &mut Context,
        rt: &mut Runtime,
        cmd: &str,
        args: &Vec<String>,
        stdin: Stdio,
        stdout: Stdio,
        pgid: Option<i32>,
        assigns: &Vec<Assign>,
    ) -> anyhow::Result<Child> {
        use std::process::Command;

        let envs = assigns.iter().map(|word| (&word.var, &word.val));

        let child = Command::new(cmd)
            .args(args)
            .stdin(stdin)
            .stdout(stdout)
            .process_group(pgid.unwrap_or(0)) // pgid of 0 means use own pid as pgid
            .current_dir(rt.working_dir.to_str().unwrap())
            .envs(envs)
            .spawn()?;

        Ok(child)
    }

    /// Small wrapper that outputs command output if exists
    fn command_output(&self, cmd_handle: Child) -> anyhow::Result<Option<Output>> {
        let cmd_output = cmd_handle.wait_with_output()?;
        print!("{}", std::str::from_utf8(&cmd_output.stdout)?);
        stdout().flush()?;
        (self.hooks.exit_code_command)(cmd_output.status.code().unwrap());
        Ok(Some(cmd_output))
    }
}

pub fn dummy_child() -> anyhow::Result<Child> {
    use std::process::Command;
    let cmd = Command::new("true").spawn()?;
    Ok(cmd)
}
