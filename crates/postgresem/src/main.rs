use std::{
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand};
use postgresem_compiler::{CompilerOptions, SemanticSnapshot, compile_lsq, normalize_lsq};

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
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommands,
    },
}

#[derive(Debug, Subcommand)]
enum QueryCommands {
    Validate {
        path: PathBuf,
    },
    Compile {
        path: PathBuf,
        #[arg(long)]
        snapshot: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum SnapshotCommands {
    Hash { path: PathBuf },
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
        Commands::Query {
            command: QueryCommands::Compile { path, snapshot },
        } => compile_query(&path, &snapshot),
        Commands::Snapshot {
            command: SnapshotCommands::Hash { path },
        } => hash_snapshot(&path),
    }
}

fn hash_snapshot(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let snapshot: SemanticSnapshot = serde_json::from_slice(&fs::read(path)?)?;
    println!("{}", snapshot.calculate_revision_hash()?);
    Ok(())
}

fn compile_query(path: &PathBuf, snapshot_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let input = fs::read(path)?;
    let snapshot: SemanticSnapshot = serde_json::from_slice(&fs::read(snapshot_path)?)?;
    let normalized = normalize_lsq(&input)?;
    let compiled = compile_lsq(&normalized, &snapshot, CompilerOptions::default())?;
    println!("{}", serde_json::to_string_pretty(&compiled)?);
    Ok(())
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
