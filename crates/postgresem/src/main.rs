use std::{collections::BTreeSet, env, error::Error, fs, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use postgresem_compiler::{
    CompilerOptions, MutationCapabilities, MutationCompilerOptions, SemanticSnapshot, compile_lsm,
    compile_lsq, diff_snapshots, normalize_lsm, normalize_lsq,
};
use serde_json::json;

mod benchmark;
mod catalog;
mod catalog_diff;
mod database;
mod doctor;
mod executor;
mod hash;
mod mcp;
mod mutation_executor;
mod published_model;
mod report;

#[derive(Debug, Parser)]
#[command(name = "postgresem", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Benchmark {
        #[command(subcommand)]
        command: BenchmarkCommands,
    },
    Catalog {
        #[command(subcommand)]
        command: CatalogCommands,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },
    Mutation {
        #[command(subcommand)]
        command: MutationCommands,
    },
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },
    Query {
        #[command(subcommand)]
        command: QueryCommands,
    },
    Report {
        #[command(subcommand)]
        command: ReportCommands,
    },
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommands,
    },
}

#[derive(Debug, Subcommand)]
enum BenchmarkCommands {
    Compiler {
        #[arg(long, default_value_t = 100)]
        models: usize,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 1000)]
        iterations: usize,
        #[arg(long, default_value_t = 50.0)]
        threshold_ms: f64,
    },
}

#[derive(Debug, Subcommand)]
enum CatalogCommands {
    Scan {
        #[arg(long, default_value = "DATABASE_URL", value_name = "NAME")]
        database_url_env: String,
    },
    Diff {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        to: PathBuf,
        #[arg(long)]
        fail_on_breaking: bool,
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
    Execute {
        path: PathBuf,
        #[arg(long)]
        project: String,
        #[arg(
            long,
            default_value = "DATABASE_URL",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        database_url_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_AUDIT_DATABASE_URL",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        audit_database_url_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_DB_ROLE",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        db_role_env: String,
    },
}

#[derive(Debug, Subcommand)]
enum ModelCommands {
    Diff {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        to: PathBuf,
        #[arg(long)]
        fail_on_breaking: bool,
    },
    Export {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "DATABASE_URL", value_name = "NAME")]
        database_url_env: String,
    },
}

#[derive(Debug, Subcommand)]
enum MutationCommands {
    Validate {
        path: PathBuf,
        #[arg(long)]
        snapshot: PathBuf,
    },
    Execute {
        path: PathBuf,
        #[arg(long)]
        project: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_MUTATION_DATABASE_URL",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        database_url_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_AUDIT_DATABASE_URL",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        audit_database_url_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_MUTATION_DB_ROLE",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        db_role_env: String,
    },
    Reconcile {
        #[arg(long)]
        project: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_IDEMPOTENCY_KEY",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        idempotency_key_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_MUTATION_DATABASE_URL",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        database_url_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_AUDIT_DATABASE_URL",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        audit_database_url_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_MUTATION_DB_ROLE",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        db_role_env: String,
    },
}

#[derive(Debug, Subcommand)]
enum McpCommands {
    Serve,
}

#[derive(Debug, Subcommand)]
enum ReportCommands {
    Beta {
        #[arg(
            long,
            default_value = "POSTGRESEM_AUDIT_DATABASE_URL",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        audit_database_url_env: String,
        #[arg(
            long,
            default_value = "POSTGRESEM_AUDIT_WRITER_PASSWORD",
            value_name = "NAME",
            value_parser = parse_environment_variable_name
        )]
        audit_password_env: String,
        #[arg(long, default_value_t = 24)]
        window_hours: u32,
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
        Commands::Benchmark {
            command:
                BenchmarkCommands::Compiler {
                    models,
                    warmup,
                    iterations,
                    threshold_ms,
                },
        } => benchmark_compiler(models, warmup, iterations, threshold_ms),
        Commands::Catalog {
            command: CatalogCommands::Scan { database_url_env },
        } => scan_catalog(&database_url_env),
        Commands::Catalog {
            command:
                CatalogCommands::Diff {
                    from,
                    to,
                    fail_on_breaking,
                },
        } => diff_catalogs(&from, &to, fail_on_breaking),
        Commands::Doctor { json } => doctor(json),
        Commands::Model {
            command:
                ModelCommands::Diff {
                    from,
                    to,
                    fail_on_breaking,
                },
        } => diff_models(&from, &to, fail_on_breaking),
        Commands::Model {
            command:
                ModelCommands::Export {
                    project,
                    database_url_env,
                },
        } => export_model(&database_url_env, &project),
        Commands::Mutation {
            command: MutationCommands::Validate { path, snapshot },
        } => validate_mutation(&path, &snapshot),
        Commands::Mutation {
            command:
                MutationCommands::Execute {
                    path,
                    project,
                    database_url_env,
                    audit_database_url_env,
                    db_role_env,
                },
        } => execute_mutation(
            &path,
            &project,
            &database_url_env,
            &audit_database_url_env,
            &db_role_env,
        ),
        Commands::Mutation {
            command:
                MutationCommands::Reconcile {
                    project,
                    idempotency_key_env,
                    database_url_env,
                    audit_database_url_env,
                    db_role_env,
                },
        } => reconcile_mutation(
            &project,
            &idempotency_key_env,
            &database_url_env,
            &audit_database_url_env,
            &db_role_env,
        ),
        Commands::Mcp {
            command: McpCommands::Serve,
        } => mcp::serve().map_err(Into::into),
        Commands::Query {
            command: QueryCommands::Validate { path },
        } => validate_query(&path),
        Commands::Query {
            command: QueryCommands::Compile { path, snapshot },
        } => compile_query(&path, &snapshot),
        Commands::Query {
            command:
                QueryCommands::Execute {
                    path,
                    project,
                    database_url_env,
                    audit_database_url_env,
                    db_role_env,
                },
        } => execute_query(
            &path,
            &project,
            &database_url_env,
            &audit_database_url_env,
            &db_role_env,
        ),
        Commands::Report {
            command:
                ReportCommands::Beta {
                    audit_database_url_env,
                    audit_password_env,
                    window_hours,
                },
        } => beta_report(&audit_database_url_env, &audit_password_env, window_hours),
        Commands::Snapshot {
            command: SnapshotCommands::Hash { path },
        } => hash_snapshot(&path),
    }
}

fn beta_report(
    audit_database_url_env: &str,
    audit_password_env: &str,
    window_hours: u32,
) -> Result<(), Box<dyn Error>> {
    let result = report::beta(audit_database_url_env, audit_password_env, window_hours)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn benchmark_compiler(
    models: usize,
    warmup: usize,
    iterations: usize,
    threshold_ms: f64,
) -> Result<(), Box<dyn Error>> {
    let result = benchmark::compiler_baseline(models, warmup, iterations, threshold_ms)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if !result.passed {
        return Err(format!(
            "compiler p95 {:.3} ms exceeded {:.3} ms threshold",
            result.p95_ms, result.threshold_ms
        )
        .into());
    }
    Ok(())
}

fn diff_models(from: &PathBuf, to: &PathBuf, fail_on_breaking: bool) -> Result<(), Box<dyn Error>> {
    let before: SemanticSnapshot = serde_json::from_slice(&fs::read(from)?)?;
    let after: SemanticSnapshot = serde_json::from_slice(&fs::read(to)?)?;
    let diff = diff_snapshots(&before, &after)?;
    println!("{}", serde_json::to_string_pretty(&diff)?);
    if fail_on_breaking && diff.has_breaking_changes() {
        return Err("semantic model diff contains breaking changes".into());
    }
    Ok(())
}

fn execute_query(
    path: &PathBuf,
    project: &str,
    database_url_env: &str,
    audit_database_url_env: &str,
    db_role_env: &str,
) -> Result<(), Box<dyn Error>> {
    let config = executor::ExecutorConfig::from_environment(
        database_url_env,
        audit_database_url_env,
        db_role_env,
    )?;
    let context = executor::ExecutionContext::new(
        format!("database-role:{}", config.database_role()),
        "cli",
    )?;
    let result = executor::execute(&fs::read(path)?, project, &config, &context)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn validate_mutation(path: &PathBuf, snapshot_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let normalized = normalize_lsm(&fs::read(path)?)?;
    let snapshot: SemanticSnapshot = serde_json::from_slice(&fs::read(snapshot_path)?)?;
    let capabilities = MutationCapabilities {
        profile: "local-validation".to_owned(),
        writable_models: snapshot
            .models
            .iter()
            .filter(|model| model.writable.is_some())
            .map(|model| model.semantic_name.clone())
            .collect::<BTreeSet<_>>(),
    };
    let compiled = compile_lsm(
        &normalized,
        &snapshot,
        &capabilities,
        MutationCompilerOptions::default(),
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": normalized.mutation.schema_version,
            "valid": true,
            "normalized_lsm_hash": normalized.hash,
            "semantic_revision": snapshot.revision_hash,
            "operation": compiled.operation,
            "model": compiled.model,
            "expected_rows": compiled.expected_rows,
            "returning_schema": compiled.returning_schema,
            "lineage": {
                "model": compiled.lineage.model,
                "fields": compiled.lineage.fields,
                "returning_fields": compiled.lineage.returning_fields,
            }
        }))?
    );
    Ok(())
}

fn execute_mutation(
    path: &PathBuf,
    project: &str,
    database_url_env: &str,
    audit_database_url_env: &str,
    db_role_env: &str,
) -> Result<(), Box<dyn Error>> {
    let config = mutation_executor::MutationExecutorConfig::from_environment(
        database_url_env,
        audit_database_url_env,
        db_role_env,
    )?;
    let context = executor::ExecutionContext::new(
        format!("database-role:{}", config.database_role()),
        "cli",
    )?;
    let result = mutation_executor::execute(&fs::read(path)?, project, &config, &context)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": result.schema_version,
            "mutation_id": result.mutation_id,
            "semantic_revision": result.semantic_revision,
            "operation": result.operation,
            "model": result.model,
            "columns": result.columns,
            "rows": result.rows,
            "affected_rows": result.affected_rows,
            "replayed": result.replayed,
            "lineage": {
                "model": result.lineage.model,
                "fields": result.lineage.fields,
                "returning_fields": result.lineage.returning_fields,
            },
            "warnings": result.warnings,
        }))?
    );
    Ok(())
}

fn reconcile_mutation(
    project: &str,
    idempotency_key_env: &str,
    database_url_env: &str,
    audit_database_url_env: &str,
    db_role_env: &str,
) -> Result<(), Box<dyn Error>> {
    let config = mutation_executor::MutationExecutorConfig::from_environment(
        database_url_env,
        audit_database_url_env,
        db_role_env,
    )?;
    let idempotency_key = env::var(idempotency_key_env)?;
    let state = mutation_executor::reconcile(project, &idempotency_key, &config)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": "1",
            "idempotency_key_hash": hash::sha256(&idempotency_key),
            "state": state
        }))?
    );
    Ok(())
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

fn diff_catalogs(
    from: &PathBuf,
    to: &PathBuf,
    fail_on_breaking: bool,
) -> Result<(), Box<dyn Error>> {
    let before: catalog::CatalogSnapshot = serde_json::from_slice(&fs::read(from)?)?;
    let after: catalog::CatalogSnapshot = serde_json::from_slice(&fs::read(to)?)?;
    let diff = catalog_diff::diff_catalogs(&before, &after)?;
    println!("{}", serde_json::to_string_pretty(&diff)?);
    if fail_on_breaking && diff.has_breaking_changes() {
        return Err("catalog diff contains breaking changes".into());
    }
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

fn doctor(json: bool) -> Result<(), Box<dyn Error>> {
    let report = doctor::inspect();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("postgresem {}", report.postgresem_version);
        println!(
            "platform: {}/{}",
            report.operating_system, report.architecture
        );
        print_runtime("Apple Container", &report.apple_container);
        print_runtime("Docker", &report.docker);
    }
    doctor::require_runtime(&report).map_err(Into::into)
}

fn print_runtime(name: &str, runtime: &doctor::RuntimeReport) {
    let state = if runtime.available {
        "available"
    } else {
        "unavailable"
    };
    println!("{name}: {state}");
    if let Some(version) = &runtime.engine_version {
        println!("  engine: {version}");
    }
    if let Some(version) = &runtime.compose_version {
        println!("  compose: {version}");
    }
}

fn validate_query(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let input = fs::read(path)?;
    let normalized = normalize_lsq(&input)?;
    println!("{}", normalized.canonical_json);
    println!("{}", normalized.hash);
    Ok(())
}

fn parse_environment_variable_name(value: &str) -> Result<String, String> {
    let mut bytes = value.bytes();
    if matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Ok(value.to_owned())
    } else {
        Err("must be an environment variable name".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{
        BenchmarkCommands, CatalogCommands, Cli, Commands, McpCommands, ModelCommands,
        MutationCommands, QueryCommands,
    };

    #[test]
    fn compiler_benchmark_defaults_to_the_preview_baseline() {
        assert!(matches!(
            Cli::try_parse_from(["postgresem", "benchmark", "compiler"]),
            Ok(Cli {
                command: Commands::Benchmark {
                    command: BenchmarkCommands::Compiler {
                        models: 100,
                        warmup: 100,
                        iterations: 1000,
                        threshold_ms,
                    }
                }
            }) if threshold_ms == 50.0
        ));
    }

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
    fn catalog_diff_has_explicit_snapshot_paths_and_breaking_gate() {
        assert!(matches!(
            Cli::try_parse_from([
                "postgresem",
                "catalog",
                "diff",
                "--from",
                "before.json",
                "--to",
                "after.json",
                "--fail-on-breaking",
            ]),
            Ok(Cli {
                command: Commands::Catalog {
                    command: CatalogCommands::Diff {
                        from,
                        to,
                        fail_on_breaking: true,
                    }
                }
            }) if from.to_str() == Some("before.json") && to.to_str() == Some("after.json")
        ));
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

    #[test]
    fn model_diff_has_explicit_snapshot_paths_and_breaking_gate() {
        assert!(matches!(
            Cli::try_parse_from([
                "postgresem",
                "model",
                "diff",
                "--from",
                "before.json",
                "--to",
                "after.json",
                "--fail-on-breaking",
            ]),
            Ok(Cli {
                command: Commands::Model {
                    command: ModelCommands::Diff {
                        from,
                        to,
                        fail_on_breaking: true,
                    }
                }
            }) if from.to_str() == Some("before.json") && to.to_str() == Some("after.json")
        ));
    }

    #[test]
    fn model_diff_breaking_gate_returns_an_error() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(
            super::diff_models(
                &root.join("tests/model-diff/before.json"),
                &root.join("tests/model-diff/after.json"),
                true,
            )
            .is_err()
        );
    }

    #[test]
    fn query_execute_accepts_only_environment_variable_names_for_guard_config() {
        let defaults = Cli::try_parse_from([
            "postgresem",
            "query",
            "execute",
            "query.json",
            "--project",
            "commerce",
        ]);
        assert!(matches!(
            defaults,
            Ok(Cli {
                command: Commands::Query {
                    command: QueryCommands::Execute {
                        database_url_env,
                        audit_database_url_env,
                        db_role_env,
                        ..
                    }
                }
            }) if database_url_env == "DATABASE_URL"
                && audit_database_url_env == "POSTGRESEM_AUDIT_DATABASE_URL"
                && db_role_env == "POSTGRESEM_DB_ROLE"
        ));

        let named = Cli::try_parse_from([
            "postgresem",
            "query",
            "execute",
            "query.json",
            "--project",
            "commerce",
            "--database-url-env",
            "RUNTIME_URL",
            "--audit-database-url-env",
            "AUDIT_URL",
            "--db-role-env",
            "MAPPED_ROLE",
        ]);
        assert!(named.is_ok());

        for forbidden in [
            vec!["--database-url", "postgresql://localhost/app"],
            vec!["--audit-database-url", "postgresql://localhost/audit"],
            vec!["--db-role", "postgresem_analyst"],
        ] {
            let mut arguments = vec![
                "postgresem",
                "query",
                "execute",
                "query.json",
                "--project",
                "commerce",
            ];
            arguments.extend(forbidden);
            assert!(Cli::try_parse_from(arguments).is_err());
        }
        assert!(
            Cli::try_parse_from([
                "postgresem",
                "query",
                "execute",
                "query.json",
                "--project",
                "commerce",
                "--database-url-env",
                "postgresql://localhost/app",
            ])
            .is_err()
        );
    }

    #[test]
    fn mutation_commands_do_not_accept_connections_roles_or_keys_as_values() {
        assert!(matches!(
            Cli::try_parse_from([
                "postgresem",
                "mutation",
                "validate",
                "mutation.json",
                "--snapshot",
                "snapshot.json"
            ]),
            Ok(Cli {
                command: Commands::Mutation {
                    command: MutationCommands::Validate { .. }
                }
            })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "postgresem",
                "mutation",
                "execute",
                "mutation.json",
                "--project",
                "commerce"
            ]),
            Ok(Cli {
                command: Commands::Mutation {
                    command: MutationCommands::Execute {
                        database_url_env,
                        audit_database_url_env,
                        db_role_env,
                        ..
                    }
                }
            }) if database_url_env == "POSTGRESEM_MUTATION_DATABASE_URL"
                && audit_database_url_env == "POSTGRESEM_AUDIT_DATABASE_URL"
                && db_role_env == "POSTGRESEM_MUTATION_DB_ROLE"
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "postgresem",
                "mutation",
                "reconcile",
                "--project",
                "commerce"
            ]),
            Ok(Cli {
                command: Commands::Mutation {
                    command: MutationCommands::Reconcile {
                        idempotency_key_env,
                        ..
                    }
                }
            }) if idempotency_key_env == "POSTGRESEM_IDEMPOTENCY_KEY"
        ));

        for forbidden in [
            vec!["--database-url", "postgresql://localhost/app"],
            vec!["--db-role", "postgresem_order_writer"],
            vec!["--idempotency-key", "secret-key"],
        ] {
            let mut arguments = vec![
                "postgresem",
                "mutation",
                "execute",
                "mutation.json",
                "--project",
                "commerce",
            ];
            arguments.extend(forbidden);
            assert!(Cli::try_parse_from(arguments).is_err());
        }
    }

    #[test]
    fn mcp_serve_has_no_request_selected_configuration() {
        assert!(matches!(
            Cli::try_parse_from(["postgresem", "mcp", "serve"]),
            Ok(Cli {
                command: Commands::Mcp {
                    command: McpCommands::Serve
                }
            })
        ));
        for forbidden in [
            "--project",
            "--database-url-env",
            "--audit-database-url-env",
            "--db-role-env",
            "--principal",
        ] {
            assert!(
                Cli::try_parse_from(["postgresem", "mcp", "serve", forbidden, "value"]).is_err()
            );
        }
    }
}
