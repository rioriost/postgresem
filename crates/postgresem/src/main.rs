use std::{
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand};
use postgresem_compiler::{CompilerOptions, SemanticSnapshot, compile_lsq, normalize_lsq};

mod catalog;
mod published_model;

#[derive(Debug, Parser)]
#[command(name = "postgresem", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Catalog {
        #[command(subcommand)]
        command: CatalogCommands,
    },
    Doctor,
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },
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
enum CatalogCommands {
    Scan {
        #[arg(long, default_value = "DATABASE_URL", value_name = "NAME")]
        database_url_env: String,
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
enum ModelCommands {
    Export {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "DATABASE_URL", value_name = "NAME")]
        database_url_env: String,
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
        Commands::Catalog {
            command: CatalogCommands::Scan { database_url_env },
        } => scan_catalog(&database_url_env),
        Commands::Doctor => doctor(),
        Commands::Model {
            command:
                ModelCommands::Export {
                    project,
                    database_url_env,
                },
        } => export_model(&database_url_env, &project),
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

fn export_model(database_url_env: &str, project: &str) -> Result<(), Box<dyn Error>> {
    let snapshot = published_model::load_from_env(database_url_env, project)?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

fn scan_catalog(database_url_env: &str) -> Result<(), Box<dyn Error>> {
    let snapshot = catalog::scan_from_env(database_url_env)?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{CatalogCommands, Cli, Commands, ModelCommands};

    #[test]
    fn catalog_scan_accepts_only_an_environment_variable_name() {
        let parsed = Cli::try_parse_from([
            "postgresem",
            "catalog",
            "scan",
            "--database-url-env",
            "POSTGRESEM_SCAN_URL",
        ]);
        assert!(matches!(
            parsed,
            Ok(Cli {
                command: Commands::Catalog {
                    command: CatalogCommands::Scan { database_url_env }
                }
            }) if database_url_env == "POSTGRESEM_SCAN_URL"
        ));

        assert!(
            Cli::try_parse_from([
                "postgresem",
                "catalog",
                "scan",
                "--database-url",
                "postgresql://localhost/app",
            ])
            .is_err()
        );
    }

    #[test]
    fn model_export_accepts_project_and_only_an_environment_variable_name() {
        let default_env =
            Cli::try_parse_from(["postgresem", "model", "export", "--project", "commerce"]);
        assert!(matches!(
            default_env,
            Ok(Cli {
                command: Commands::Model {
                    command: ModelCommands::Export {
                        project,
                        database_url_env
                    }
                }
            }) if project == "commerce" && database_url_env == "DATABASE_URL"
        ));

        let parsed = Cli::try_parse_from([
            "postgresem",
            "model",
            "export",
            "--project",
            "commerce",
            "--database-url-env",
            "POSTGRESEM_MODEL_URL",
        ]);
        assert!(matches!(
            parsed,
            Ok(Cli {
                command: Commands::Model {
                    command: ModelCommands::Export {
                        project,
                        database_url_env
                    }
                }
            }) if project == "commerce" && database_url_env == "POSTGRESEM_MODEL_URL"
        ));

        assert!(
            Cli::try_parse_from([
                "postgresem",
                "model",
                "export",
                "--project",
                "commerce",
                "--database-url",
                "postgresql://localhost/app",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "postgresem",
                "model",
                "export",
                "--project",
                "commerce",
                "postgresql://localhost/app",
            ])
            .is_err()
        );
    }
}
