use std::path::PathBuf;

use postgresem_compiler::{CompilerOptions, Literal, SemanticSnapshot, compile_lsq, normalize_lsq};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct EvalSuite {
    cases: Vec<EvalCase>,
}

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
    dataset: String,
    question: String,
    lsq: Value,
    expect: Expectation,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum Expectation {
    Accept {
        output: Vec<String>,
        parameter_count: usize,
    },
    Reject {
        code: String,
    },
}

#[test]
fn m0_suite_has_three_datasets_and_thirty_cases() {
    let suite = load_suite();
    let datasets = suite
        .cases
        .iter()
        .map(|case| case.dataset.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(suite.cases.len(), 30);
    assert_eq!(
        datasets,
        ["commerce", "rls-multitenant", "subscriptions"]
            .into_iter()
            .collect()
    );
}

#[test]
fn m0_queries_compile_or_fail_closed_as_expected() {
    let suite = load_suite();
    let snapshot = load_snapshot();

    for case in suite.cases {
        assert_eq!(evaluate_case(&case, &snapshot), Ok(()));
    }
}

fn evaluate_case(case: &EvalCase, snapshot: &SemanticSnapshot) -> Result<(), String> {
    if case.question.trim().is_empty() {
        return Err(format!("{} has no question", case.id));
    }
    let input = serde_json::to_vec(&case.lsq).map_err(|error| format!("{}: {error}", case.id))?;
    let normalized = match normalize_lsq(&input) {
        Ok(normalized) => normalized,
        Err(error) => {
            return match &case.expect {
                Expectation::Reject { code } if error.code() == code => Ok(()),
                Expectation::Reject { code } => Err(format!(
                    "{} rejected with {}, expected {code}",
                    case.id,
                    error.code()
                )),
                Expectation::Accept { .. } => Err(format!(
                    "{} unexpectedly failed LSQ validation: {error}",
                    case.id
                )),
            };
        }
    };

    match (
        compile_lsq(&normalized, snapshot, CompilerOptions::default()),
        &case.expect,
    ) {
        (
            Ok(compiled),
            Expectation::Accept {
                output,
                parameter_count,
            },
        ) => {
            let actual_output = compiled
                .output_schema
                .iter()
                .map(|column| column.name.clone())
                .collect::<Vec<_>>();
            if &actual_output != output {
                return Err(format!(
                    "{} returned output {actual_output:?}, expected {output:?}",
                    case.id
                ));
            }
            if compiled.parameters.len() != *parameter_count {
                return Err(format!(
                    "{} returned {} parameters, expected {parameter_count}",
                    case.id,
                    compiled.parameters.len()
                ));
            }
            if compiled.sql.contains(';') {
                return Err(format!("{} generated multiple-statement syntax", case.id));
            }
            for (index, parameter) in compiled.parameters.iter().enumerate() {
                if parameter.position != index + 1
                    || !compiled.sql.contains(&format!("${}", parameter.position))
                {
                    return Err(format!(
                        "{} generated inconsistent parameter positions",
                        case.id
                    ));
                }
                let sensitive_value = match &parameter.value {
                    Literal::Text(value)
                    | Literal::Numeric(value)
                    | Literal::Date(value)
                    | Literal::Timestamp(value) => Some(value),
                    Literal::Boolean(_) | Literal::Integer(_) => None,
                };
                if sensitive_value.is_some_and(|value| compiled.sql.contains(value)) {
                    return Err(format!(
                        "{} embedded a parameter value in generated SQL",
                        case.id
                    ));
                }
            }
            let repeated = compile_lsq(&normalized, snapshot, CompilerOptions::default())
                .map_err(|error| format!("{} failed repeated compilation: {error}", case.id))?;
            if compiled != repeated {
                return Err(format!("{} compilation is not deterministic", case.id));
            }
            Ok(())
        }
        (Ok(_), Expectation::Reject { code }) => Err(format!(
            "{} unexpectedly compiled; expected {code}",
            case.id
        )),
        (Err(error), Expectation::Reject { code }) if error.code() == code => Ok(()),
        (Err(error), Expectation::Reject { code }) => Err(format!(
            "{} rejected with {}, expected {code}",
            case.id,
            error.code()
        )),
        (Err(error), Expectation::Accept { .. }) => Err(format!(
            "{} unexpectedly failed semantic validation: {error}",
            case.id
        )),
    }
}

fn load_suite() -> EvalSuite {
    serde_json::from_slice(
        &std::fs::read(repo_path("fixtures/evals/m0-evals.json")).expect("M0 eval fixture exists"),
    )
    .expect("valid M0 eval fixture")
}

fn load_snapshot() -> SemanticSnapshot {
    serde_json::from_slice(
        &std::fs::read(repo_path("fixtures/evals/m0-semantic-snapshot.json"))
            .expect("semantic snapshot fixture exists"),
    )
    .expect("valid semantic snapshot fixture")
}

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}
