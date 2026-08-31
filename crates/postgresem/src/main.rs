use std::{
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand};
use postgresem_compiler::normalize_lsq;

#[derive(Debug, Parser)]
#[command(name = "postgresem", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Doctor,
    Query {
        #[command(subcommand)]
        command: QueryCommands,
    },
}

#[derive(Debug, Subcommand)]
enum QueryCommands {
    Validate { path: PathBuf },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match Cli::parse().command {
        Commands::Doctor => doctor(),
        Commands::Query {
            command: QueryCommands::Validate { path },
        } => validate_query(&path),
    }
}

fn doctor() -> Result<(), Box<dyn Error>> {
    println!("postgresem {}", env!("CARGO_PKG_VERSION"));
    for command in ["container", "container-compose"] {
        let output = Command::new(command).arg("--version").output()?;
        if !output.status.success() {
            return Err(format!("{command} --version failed").into());
        }
        let version = String::from_utf8(output.stdout)?;
        println!("{}", version.trim());
    }
    Ok(())
}

fn validate_query(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let input = fs::read(path)?;
    let normalized = normalize_lsq(&input)?;
    println!("{}", normalized.canonical_json);
    println!("{}", normalized.hash);
    Ok(())
}
