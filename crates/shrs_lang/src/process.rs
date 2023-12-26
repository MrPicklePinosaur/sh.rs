//! Process management

use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    io::{stdin, Stdin},
    os::fd::{AsRawFd, RawFd},
    process::exit,
};

use nix::{
    libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, TCSADRAIN, WNOHANG, WUNTRACED},
    sys::{
        signal::{
            kill, signal, sigprocmask, SigHandler, SigmaskHow,
            Signal::{self, SIGCONT, SIGTTIN},
        },
        signalfd::SigSet,
        termios::{tcgetattr, tcsetattr, SetArg, Termios},
        wait::{waitpid, WaitPidFlag, WaitStatus},
    },
    unistd::{
        close, dup2, execvp, fork, getpgrp, getpid, isatty, setpgid, tcgetpgrp, tcsetpgrp,
        ForkResult, Pid,
    },
};

/// A single OS process
pub struct Process {
    /// Process id
    pub pid: Pid,
    /// List of args to be passed to process
    pub argv: Vec<String>,
}

/// Unique identifier to keep track of job
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct JobId(pub usize);

/// A job corresponds to a pipeline of processes
pub struct Job {
    pub jobid: JobId,
    /// Process group id
    pub pgid: Pid,
    /// All of the processes in this job
    pub processes: Vec<Pid>,
}

/// Execution context for a process
pub struct Context {
    pub stdin: RawFd,
    pub stdout: RawFd,
    pub stderr: RawFd,
    /// Is the current job running in the foreground
    pub is_foreground: bool,
    /// Is the shell in interactive mode
    pub is_interactive: bool,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ProcessState {
    Running,
    Exited(i32),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExitStatus {
    Exited(i32),
    Running(Pid),
}

pub enum Pgid {
    /// Pgid of current corresponds to using the same Pgid as the current group is using
    Current,
    /// A specific Pgid
    Pgid(Pid),
}

// Run a command
pub fn run_process(
    argv: &[String],
    pgid: Pgid,
    ctx: &Context,
) -> Result<ExitStatus, std::io::Error> {
    // fork the child
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => Ok(ExitStatus::Running(child)),
        Ok(ForkResult::Child) => {
            setup_process(argv, pgid, ctx)?;
            unreachable!()
        },
        Err(_) => todo!(),
    }
}

// Code to run in child after new process is forked
fn setup_process(argv: &[String], pgid: Pgid, ctx: &Context) -> Result<(), std::io::Error> {
    // If interactive need to give the current process control of the tty
    let shell_term = STDIN_FILENO;
    if ctx.is_interactive {
        let pid = getpid();
        let new_pgid = match pgid {
            Pgid::Current => pid,
            Pgid::Pgid(pgid) => pgid,
        };
        setpgid(pid, new_pgid)?;

        // If process is being launched by foreground job, we also need the process to be in
        // the foreground
        if ctx.is_foreground {
            tcsetpgrp(shell_term, new_pgid)?;
        }

        // Reset signals
        unsafe {
            signal(Signal::SIGINT, SigHandler::SigIgn);
            signal(Signal::SIGQUIT, SigHandler::SigIgn);
            signal(Signal::SIGTSTP, SigHandler::SigIgn);
            signal(Signal::SIGTTIN, SigHandler::SigIgn);
            signal(Signal::SIGTTOU, SigHandler::SigIgn);
            signal(Signal::SIGCHLD, SigHandler::SigIgn);
        };
    }

    // Set stdio of new process
    if ctx.stdin != STDIN_FILENO {
        dup2(ctx.stdin, STDIN_FILENO)?;
        close(ctx.stdin)?;
    }
    if ctx.stdout != STDOUT_FILENO {
        dup2(ctx.stdout, STDOUT_FILENO)?;
        close(ctx.stdout)?;
    }
    if ctx.stderr != STDERR_FILENO {
        dup2(ctx.stderr, STDERR_FILENO)?;
        close(ctx.stderr)?;
    }

    // We can fork now
    let filename = argv.get(0).unwrap();
    let args = argv
        .iter()
        .map(|s| CString::new(s.clone()).unwrap())
        .collect::<Vec<_>>();
    execvp(&CString::new(filename.clone()).unwrap(), &args)?;
    exit(1);
}

impl Job {
    /// Check job has completed
    ///
    /// Jobs are completed when all the processes in the job has completed
    pub fn exited(&self, os: &Os) -> bool {
        self.processes.iter().all(|pid| {
            let state = os.get_process_state(pid).expect("missing process");
            matches!(state, ProcessState::Exited(_))
        })
    }

    /// Get the state of the last process in the job
    pub fn last_process_state(&self, os: &Os) -> Option<ProcessState> {
        self.processes
            .iter()
            .last()
            .map(|pid| os.get_process_state(pid).expect("missing process").clone())
    }
}

/*
/// Store context related to jobs
pub struct JobMap {

}

/// Store status of all processes
pub struct ProcMap {

}
*/

/// Context related to state of processes and jobs
pub struct Os {
    pgid: Pid,
    tmods: Termios,
    jobs: HashMap<JobId, Job>,
    proc_state: HashMap<Pid, ProcessState>,
}

impl Os {
    /// Initialize job control for the shell
    pub fn init_shell() -> Result<Self, std::io::Error> {
        // Check if the current shell is allowed to run it's own job control
        let shell_term = STDIN_FILENO;

        if !isatty(shell_term)? {
            // return Ok(());
            panic!("Not interactive")
        }

        // Wait until parent puts us into foreground
        while tcgetpgrp(shell_term)? != getpgrp() {
            // SIGTTIN tells process to suspend since it's not in foreground
            kill(getpgrp(), SIGTTIN)?;
        }

        // Ignore interactive and job control signals
        // TODO double check correctness of unsafe code
        unsafe {
            signal(Signal::SIGINT, SigHandler::SigIgn);
            signal(Signal::SIGQUIT, SigHandler::SigIgn);
            signal(Signal::SIGTSTP, SigHandler::SigIgn);
            signal(Signal::SIGTTIN, SigHandler::SigIgn);
            signal(Signal::SIGTTOU, SigHandler::SigIgn);
            signal(Signal::SIGCHLD, SigHandler::SigIgn);
        };

        // Put self in own process group
        let pgid = getpid();
        setpgid(pgid, pgid)?;
        tcsetpgrp(shell_term, pgid)?;

        let tmods = tcgetattr(shell_term)?;

        let os = Os {
            pgid,
            tmods,
            jobs: HashMap::new(),
            proc_state: HashMap::new(),
        };
        Ok(os)
    }

    pub fn shell_pgid(&self) -> Pid {
        self.pgid
    }

    // JOB RELATED
    pub fn create_job(&mut self, pgid: Pid, processes: Vec<Pid>) -> Result<JobId, std::io::Error> {
        let jobid = self.find_free_job_id();
        let new_job = Job {
            jobid: jobid.clone(),
            pgid,
            processes,
        };
        self.jobs.insert(jobid.clone(), new_job);
        Ok(jobid)
    }

    fn find_free_job_id(&self) -> JobId {
        let mut id = 1usize;
        while self.jobs.contains_key(&JobId(id)) {
            id += 1;
        }
        JobId(id)
    }

    /// Wait for entire job to finish
    pub fn wait_for_job(&mut self, jobid: JobId) -> Result<ProcessState, std::io::Error> {
        loop {
            // TODO throw proper error here
            let job = self.jobs.get(&jobid).expect("non existent jobid");
            if job.exited(self) {
                break;
            }
            self.wait_for_any_process()?;
        }
        // remove from tracked job list
        let job = self.jobs.get(&jobid).expect("non existent jobid");
        let process_state = job.last_process_state(self).unwrap();
        match process_state {
            ProcessState::Exited(status) => {
                self.remove_job(&jobid);
                Ok(process_state)
            },
            _ => unreachable!(),
        }
    }

    /// Block until any process terminates
    fn wait_for_any_process(&mut self) -> Result<Option<Pid>, std::io::Error> {
        // PID of None means wait for any child process
        let wait_status = waitpid(None, WaitPidFlag::from_bits(WUNTRACED | WNOHANG))?;
        match wait_status {
            WaitStatus::Exited(pid, status) => {
                self.set_process_state(pid, ProcessState::Exited(status));
                Ok(Some(pid))
            },
            WaitStatus::StillAlive => Ok(None),
            _ => todo!(),
        }
    }

    fn set_process_state(&mut self, pid: Pid, state: ProcessState) {
        self.proc_state.insert(pid, state);
    }
    pub fn get_process_state(&self, pid: &Pid) -> Option<&ProcessState> {
        self.proc_state.get(pid)
    }

    fn remove_job(&mut self, jobid: &JobId) {
        self.jobs.remove(jobid);
    }

    /// Place job onto foreground
    pub fn run_in_foreground(
        &mut self,
        jobid: JobId,
        cont: bool,
    ) -> Result<ProcessState, std::io::Error> {
        let shell_term = STDIN_FILENO;

        let job = self.jobs.get(&jobid).unwrap();

        // Put the job into foreground
        tcsetpgrp(shell_term, job.pgid)?;

        // TODO also run tcsetattr
        // Send job continue signal
        if cont {
            kill(job.pgid, SIGCONT)?;
        }

        // Wait for the job
        let proc_state = self.wait_for_job(jobid)?;

        // Return foreground to the shell
        tcsetpgrp(shell_term, self.shell_pgid())?;

        // TODO restore terminal mode
        tcsetattr(shell_term, SetArg::TCSADRAIN, &self.tmods)?;

        Ok(proc_state)
    }

    /// Place job onto background
    pub fn run_in_background(&self, jobid: JobId, cont: bool) -> Result<(), std::io::Error> {
        if cont {
            let job = self.jobs.get(&jobid).unwrap();
            kill(job.pgid, SIGCONT)?;
        }
        Ok(())
    }
}
