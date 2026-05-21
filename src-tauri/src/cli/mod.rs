use std::{io::Write, process::exit, thread, time::Duration};

use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::{
    core::{AddProjectOptions, DomainestCore},
    error::AppError,
    models::Project,
};

#[derive(Parser)]
#[command(name = "domainest", about = "Local dev domains for your projects")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List registered projects
    List {
        #[arg(long)]
        json: bool,
    },
    /// Add a project from a directory path
    Add {
        path: String,
        #[arg(long)]
        domain: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        no_ssl: bool,
    },
    /// Start a project's dev server
    Start { project: String },
    /// Stop a project's dev server
    Stop { project: String },
    /// Remove a project
    Remove {
        project: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Open project URL in the default browser
    Open { project: String },
    /// Show project logs
    Logs {
        project: String,
        #[arg(long, default_value = "80000")]
        bytes: u64,
        #[arg(short, long)]
        follow: bool,
    },
    /// Show overall status
    Status {
        #[arg(long)]
        json: bool,
    },
    /// DNS zone configuration
    Zone {
        #[command(subcommand)]
        command: ZoneCommands,
    },
    /// Sync macOS DNS resolvers and embedded DNS
    Dns {
        #[command(subcommand)]
        command: Option<DnsCommands>,
    },
}

#[derive(Subcommand)]
pub enum ZoneCommands {
    /// Print the current DNS zone
    Get,
    /// Set the DNS zone (e.g. test, myapp.com)
    Set { zone: String },
}

#[derive(Subcommand)]
pub enum DnsCommands {
    /// Apply DNS configuration for all projects
    Sync,
}

#[derive(Serialize)]
struct StatusJson {
    zone: String,
    running_count: usize,
    projects: Vec<Project>,
}

pub fn run() {
    let cli = Cli::parse();
    let core = match DomainestCore::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            exit(2);
        }
    };

    let result = dispatch(&core, cli.command);
    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{e}");
            let code = match &e {
                AppError::ToolMissing { .. } | AppError::ToolFailed { .. } => 2,
                _ => 1,
            };
            exit(code);
        }
    }
}

fn dispatch(core: &DomainestCore, command: Commands) -> Result<(), AppError> {
    match command {
        Commands::List { json } => {
            let projects = core.list_projects()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&projects)?);
            } else {
                print_projects_table(&projects);
            }
        }
        Commands::Add {
            path,
            domain,
            port,
            no_ssl,
        } => {
            let project = core.add_project(
                path,
                AddProjectOptions {
                    domain,
                    port,
                    ssl: if no_ssl { Some(false) } else { None },
                },
            )?;
            println!(
                "Added {} ({}) on port {}",
                project.name, project.domain, project.port
            );
        }
        Commands::Start { project } => {
            let p = core.start_project(&project)?;
            println!(
                "Started {} → http{}://{}",
                p.name,
                if p.ssl { "s" } else { "" },
                p.domain
            );
        }
        Commands::Stop { project } => {
            let p = core.stop_project(&project)?;
            println!("Stopped {}", p.name);
        }
        Commands::Remove { project, yes } => {
            if !yes {
                eprintln!("Use -y/--yes to remove `{project}`");
                return Err(AppError::ToolFailed {
                    tool: "remove".to_string(),
                    message: "confirmation required".to_string(),
                });
            }
            core.remove_project(&project)?;
            println!("Removed {project}");
        }
        Commands::Open { project } => {
            core.open_project(&project)?;
        }
        Commands::Logs {
            project,
            bytes,
            follow,
        } => {
            if follow {
                tail_logs(core, &project, bytes)?;
            } else {
                let content = core.read_log(&project, bytes)?;
                print!("{content}");
            }
        }
        Commands::Status { json } => {
            let status = core.status()?;
            if json {
                let out = StatusJson {
                    zone: status.zone,
                    running_count: status.running_count,
                    projects: status.projects,
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("DNS zone: {}", status.zone);
                println!(
                    "Projects: {} total, {} running",
                    status.projects.len(),
                    status.running_count
                );
                print_projects_table(&status.projects);
            }
        }
        Commands::Zone { command } => match command {
            ZoneCommands::Get => {
                println!("{}", core.get_zone()?);
            }
            ZoneCommands::Set { zone } => {
                let z = core.set_zone(&zone)?;
                println!("DNS zone set to {z}");
            }
        },
        Commands::Dns { command } => match command {
            Some(DnsCommands::Sync) | None => {
                core.sync_dns()?;
                println!("DNS synced");
            }
        },
    }
    Ok(())
}

fn tail_logs(core: &DomainestCore, project: &str, bytes: u64) -> Result<(), AppError> {
    let mut last_len = 0usize;
    loop {
        let content = core.read_log(project, bytes)?;
        if content.len() > last_len {
            let _ = std::io::stdout().write_all(content[last_len..].as_bytes());
            let _ = std::io::stdout().flush();
            last_len = content.len();
        }
        thread::sleep(Duration::from_millis(1200));
    }
}

fn print_projects_table(projects: &[Project]) {
    if projects.is_empty() {
        println!("No projects.");
        return;
    }
    println!(
        "{:<36}  {:<16}  {:<6}  {:<8}  {}",
        "ID", "NAME", "PORT", "STATUS", "DOMAIN"
    );
    for p in projects {
        let status = match p.status {
            crate::models::ProjectStatus::Running => "running",
            crate::models::ProjectStatus::Stopped => "stopped",
        };
        println!(
            "{:<36}  {:<16}  {:<6}  {:<8}  {}",
            p.id, p.name, p.port, status, p.domain
        );
    }
}
