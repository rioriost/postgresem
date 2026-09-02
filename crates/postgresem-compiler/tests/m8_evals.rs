use std::path::PathBuf;

use postgresem_compiler::{CompilerOptions, SemanticSnapshot, compile_lsq, normalize_lsq};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct EvalSuite {
    cases: Vec<EvalCase>,
}

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
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
fn m8_suite_balances_accepted_and_rejected_fanout_cases() {
    let suite = load_suite();
    let accepted = suite
        .cases
        .iter()
        .filter(|case| matches!(case.expect, Expectation::Accept { .. }))
        .count();
    assert_eq!(suite.cases.len(), 13);
    assert_eq!(accepted, 6);
}

#[test]
fn m8_queries_compile_or_fail_closed_as_expected() {
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
    let input = serde_json::to_vec(&case.lsq).map_err(|error| error.to_string())?;
    let normalized = normalize_lsq(&input).map_err(|error| error.to_string())?;
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
            assert_eq!(
                compiled
                    .output_schema
                    .iter()
                    .map(|column| column.name.clone())
                    .collect::<Vec<_>>()
                    .as_slice(),
                output.as_slice(),
                "{} output mismatch",
                case.id
            );
            assert_eq!(
                compiled.parameters.len(),
                *parameter_count,
                "{} parameter count mismatch",
                case.id
            );
            assert!(
                compiled
                    .sql
                    .starts_with("WITH \"__postgresem_anchor_groups\""),
                "{} did not use the anchored plan",
                case.id
            );
            assert!(
                !compiled.sql.contains(';'),
                "{} emitted a semicolon",
                case.id
            );
            assert_eq!(
                compiled,
                compile_lsq(&normalized, snapshot, CompilerOptions::default())
                    .expect("repeated compilation succeeds"),
                "{} compilation is not deterministic",
                case.id
            );
            Ok(())
        }
        (Err(error), Expectation::Reject { code }) => {
            if error.code() == code {
                Ok(())
            } else {
                Err(format!(
                    "{} rejected with {}, expected {code}",
                    case.id,
                    error.code()
                ))
            }
        }
        (Ok(_), Expectation::Reject { code }) => {
            Err(format!("{} compiled but expected {code}", case.id))
        }
        (Err(error), Expectation::Accept { .. }) => {
            Err(format!("{} failed unexpectedly: {error}", case.id))
        }
    }
}

#[test]
fn duplicate_safe_plan_has_stable_sql_and_lineage() {
    let snapshot = load_snapshot();
    let normalized = normalize_lsq(
        br#"{
          "schema_version":"1",
          "model":"orders",
          "dimensions":[{"field":"item_sku"}],
          "metrics":[{"metric":"revenue"}],
          "order_by":[{"ref":"item_sku","direction":"asc"}]
        }"#,
    )
    .expect("valid query");
    let compiled = compile_lsq(&normalized, &snapshot, CompilerOptions::default())
        .expect("anchored query compiles");

    assert_eq!(
        compiled.sql,
        concat!(
            "WITH \"__postgresem_anchor_groups\" AS (\n",
            "  SELECT t1.\"sku\" AS \"__d0\", t0.\"order_id\" AS \"__anchor\", ",
            "max(CASE WHEN t0.\"status\" = $1::text THEN t0.\"amount\" END) AS \"__m0\"\n",
            "  FROM \"commerce\".\"orders\" AS t0\n",
            "  LEFT JOIN \"commerce\".\"order_item\" AS t1 ON t0.\"order_id\" = t1.\"order_id\"\n",
            "  GROUP BY 1, 2\n",
            ")\n",
            "SELECT a.\"__d0\" AS \"item_sku\", sum(a.\"__m0\") AS \"revenue\"\n",
            "FROM \"__postgresem_anchor_groups\" AS a\n",
            "GROUP BY 1\n",
            "ORDER BY \"item_sku\" ASC\n",
            "LIMIT $2::text::bigint"
        )
    );
    assert_eq!(compiled.lineage.models, ["order_items", "orders"]);
    assert_eq!(compiled.lineage.relationships, ["items"]);
    assert_eq!(compiled.lineage.aggregation_anchors.len(), 1);
    assert_eq!(compiled.lineage.aggregation_anchors[0].field, "order_id");
    assert_eq!(
        compiled.lineage.source_columns,
        [
            "commerce.order_item.order_id",
            "commerce.order_item.sku",
            "commerce.orders.amount",
            "commerce.orders.order_id",
            "commerce.orders.status"
        ]
    );
}

fn load_suite() -> EvalSuite {
    serde_json::from_slice(
        &std::fs::read(repo_path("fixtures/evals/m8-evals.json")).expect("M8 eval fixture exists"),
    )
    .expect("valid M8 eval fixture")
}

fn load_snapshot() -> SemanticSnapshot {
    serde_json::from_slice(
        &std::fs::read(repo_path("fixtures/evals/m8-semantic-snapshot.json"))
            .expect("M8 semantic snapshot exists"),
    )
    .expect("valid M8 semantic snapshot")
}

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}
