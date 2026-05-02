use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

use nix::sys::signal::Signal;
use nix::unistd::Pid;
use uuid::Uuid;

use crate::{error::AppResult, models::Project, paths};

pub struct ProcessManager {
    children: Mutex<HashMap<Uuid, ManagedChild>>,
}

struct ManagedChild {
    child: Child,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            children: Mutex::new(HashMap::new()),
        }
    }

    pub fn spawn_dev_server(&self, project: &Project) -> AppResult<u32> {
        let logs_dir = paths::logs_dir()?;
        fs::create_dir_all(&logs_dir)?;

        let log_path = logs_dir.join(format!("{}.log", project.id));
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let log_file_err = log_file.try_clone()?;

        let (args, envs) = compute_command_args_and_env(project);

        let mut cmd = Command::new(&project.command);
        cmd.args(args)
            .current_dir(&project.path)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err))
            .stdin(Stdio::null());

        for (k, v) in envs {
            cmd.env(k, v);
        }

        // Put the dev server in its own process group so we can reliably terminate
        // the whole tree (pnpm -> node -> watchers) on stop.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    // setpgid(0, 0): make this child the leader of a new process group.
                    // Safety: called in the child just before exec.
                    libc::setpgid(0, 0);
                    Ok(())
                });
            }
        }

        let child = cmd.spawn()?;
        let pid = child.id();

        let mut children = self.children.lock().unwrap();
        children.insert(project.id, ManagedChild { child });

        Ok(pid)
    }

    pub fn stop(&self, project_id: Uuid, timeout: Duration) -> AppResult<()> {
        let mut children = self.children.lock().unwrap();
        let Some(mut managed) = children.remove(&project_id) else {
            return Ok(());
        };

        let pid_u32 = managed.child.id();

        // Prefer killing the entire process group (in case the parent spawned children).
        #[cfg(unix)]
        {
            use nix::sys::signal::killpg;
            let pgid = Pid::from_raw(pid_u32 as i32);
            let _ = killpg(pgid, Signal::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            let pid = Pid::from_raw(pid_u32 as i32);
            let _ = nix_kill(pid, Signal::SIGTERM);
        }

        let deadline = Instant::now() + timeout;
        loop {
            if let Some(_status) = managed.child.try_wait()? {
                return Ok(());
            }

            if Instant::now() >= deadline {
                break;
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        #[cfg(unix)]
        {
            use nix::sys::signal::killpg;
            let pgid = Pid::from_raw(pid_u32 as i32);
            let _ = killpg(pgid, Signal::SIGKILL);
        }

        let _ = managed.child.kill();
        let _ = managed.child.wait();
        Ok(())
    }

    pub fn stop_by_pid(&self, pid: u32, timeout: Duration) -> AppResult<()> {
        #[cfg(unix)]
        {
            use nix::sys::signal::killpg;
            let pgid = Pid::from_raw(pid as i32);
            let _ = killpg(pgid, Signal::SIGTERM);
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                // kill(…, 0) would require additional handling; best-effort short sleep.
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = killpg(pgid, Signal::SIGKILL);
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = (pid, timeout);
            Ok(())
        }
    }
}

fn compute_command_args_and_env(project: &Project) -> (Vec<String>, Vec<(String, String)>) {
    let mut args = project.args.clone();
    let envs = vec![
        ("PORT".to_string(), project.port.to_string()),
        ("HOST".to_string(), "127.0.0.1".to_string()),
    ];

    // If the user uses pnpm/npm/yarn/bun to run a dev script, we can usually forward the port flag
    // to the underlying tool via `--`.
    let tool = project.command.to_lowercase();
    let is_script_runner = tool == "pnpm" || tool == "npm" || tool == "yarn" || tool == "bun";

    if is_script_runner {
        let has_port_flag = args
            .iter()
            .any(|a| a == "--port" || a.starts_with("--port=") || a == "-p");
        if !has_port_flag {
            let sep = args.iter().position(|a| a == "--");
            match sep {
                Some(i) => {
                    args.insert(i + 1, "--port".to_string());
                    args.insert(i + 2, project.port.to_string());
                }
                None => {
                    args.push("--".to_string());
                    args.push("--port".to_string());
                    args.push(project.port.to_string());
                }
            }
        }
    }

    (args, envs)
}
