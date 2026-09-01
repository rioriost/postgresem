use std::time::Instant;

use postgresem_compiler::{
    Aggregation, CompilerOptions, DataType, Field, Metric, Model, Relation, SemanticSnapshot,
    compile_lsq, normalize_lsq,
};
use serde::Serialize;
use thiserror::Error;

const BENCHMARK_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Serialize)]
pub struct CompilerBenchmark {
    pub schema_version: String,
    pub model_count: usize,
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
    pub threshold_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub passed: bool,
}

#[derive(Debug, Error)]
pub enum BenchmarkError {
    #[error("benchmark model and iteration counts must be positive")]
    InvalidConfiguration,
    #[error("benchmark model count is too large")]
    ModelCountTooLarge,
    #[error(transparent)]
    SnapshotHash(#[from] postgresem_compiler::SnapshotHashError),
    #[error(transparent)]
    Lsq(#[from] postgresem_compiler::LsqError),
    #[error(transparent)]
    Compile(#[from] postgresem_compiler::CompileError),
}

pub fn compiler_baseline(
    model_count: usize,
    warmup_iterations: usize,
    measured_iterations: usize,
    threshold_ms: f64,
) -> Result<CompilerBenchmark, BenchmarkError> {
    if model_count == 0
        || warmup_iterations == 0
        || measured_iterations == 0
        || !threshold_ms.is_finite()
        || threshold_ms <= 0.0
    {
        return Err(BenchmarkError::InvalidConfiguration);
    }
    if model_count > 10_000 {
        return Err(BenchmarkError::ModelCountTooLarge);
    }

    let snapshot = synthetic_snapshot(model_count)?;
    let query = format!(
        r#"{{"schema_version":"1","model":"model_{:05}","metrics":[{{"metric":"row_count"}}],"limit":100}}"#,
        model_count - 1
    );

    for _ in 0..warmup_iterations {
        compile_once(query.as_bytes(), &snapshot)?;
    }

    let mut samples = Vec::with_capacity(measured_iterations);
    for _ in 0..measured_iterations {
        let started = Instant::now();
        compile_once(query.as_bytes(), &snapshot)?;
        samples.push(started.elapsed().as_nanos());
    }
    samples.sort_unstable();

    let p50_ms = nanos_to_milliseconds(percentile(&samples, 50));
    let p95_ms = nanos_to_milliseconds(percentile(&samples, 95));
    let max_ms = nanos_to_milliseconds(*samples.last().unwrap_or(&0));

    Ok(CompilerBenchmark {
        schema_version: BENCHMARK_SCHEMA_VERSION.to_owned(),
        model_count,
        warmup_iterations,
        measured_iterations,
        threshold_ms,
        p50_ms,
        p95_ms,
        max_ms,
        passed: p95_ms < threshold_ms,
    })
}

fn compile_once(input: &[u8], snapshot: &SemanticSnapshot) -> Result<(), BenchmarkError> {
    let normalized = normalize_lsq(input)?;
    compile_lsq(&normalized, snapshot, CompilerOptions::default())?;
    Ok(())
}

fn synthetic_snapshot(model_count: usize) -> Result<SemanticSnapshot, BenchmarkError> {
    let mut snapshot = SemanticSnapshot {
        schema_version: "1".to_owned(),
        revision_hash: String::new(),
        models: (0..model_count)
            .map(|index| {
                let name = format!("model_{index:05}");
                Model {
                    semantic_name: name.clone(),
                    source: Relation {
                        schema: "preview".to_owned(),
                        relation: name,
                    },
                    timezone: Some("UTC".to_owned()),
                    queryable: true,
                    writable: None,
                    fields: vec![
                        Field {
                            semantic_name: "id".to_owned(),
                            data_type: DataType::Integer,
                            column: "id".to_owned(),
                            relationship: None,
                            time_dimension: false,
                            entity_key: true,
                            visible: true,
                            nullable: false,
                        },
                        Field {
                            semantic_name: "created_at".to_owned(),
                            data_type: DataType::TimestampTz,
                            column: "created_at".to_owned(),
                            relationship: None,
                            time_dimension: true,
                            entity_key: false,
                            visible: true,
                            nullable: false,
                        },
                    ],
                    metrics: vec![Metric {
                        semantic_name: "row_count".to_owned(),
                        data_type: DataType::Integer,
                        aggregation: Aggregation::Count,
                        field: "id".to_owned(),
                        filter: None,
                        visible: true,
                    }],
                    relationships: vec![],
                }
            })
            .collect(),
    };
    snapshot.revision_hash = snapshot.calculate_revision_hash()?;
    Ok(snapshot)
}

fn percentile(samples: &[u128], percentile: usize) -> u128 {
    let rank = samples.len().saturating_mul(percentile).saturating_add(99) / 100;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

fn nanos_to_milliseconds(nanos: u128) -> f64 {
    nanos as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::{compiler_baseline, percentile};

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 50), 3);
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 95), 5);
    }

    #[test]
    fn compiler_baseline_covers_one_hundred_models() {
        let result = compiler_baseline(100, 2, 10, 50.0).expect("benchmark runs");
        assert_eq!(result.model_count, 100);
        assert!(result.p95_ms >= 0.0);
    }
}
