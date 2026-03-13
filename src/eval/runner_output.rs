use std::path::Path;

use crate::eval::metrics::compute_eval_metrics;
use crate::eval::report::{write_junit, write_results, write_summary_md};
use crate::eval::types::{
    EvalConfig, EvalMetricDirection, EvalMetricRow, EvalResults, EvalRunRow, ModelSummary,
    TaskSummary,
};

pub(crate) fn finalize_and_write_eval_results(
    config: &EvalConfig,
    out_path: &Path,
    results: &mut EvalResults,
) -> anyhow::Result<()> {
    finalize_summary(results);
    results.ux_summary_metric_rows = compute_ux_summary_metric_rows(&results.runs);
    results.ux_summary_metric_rows_by_model = compute_ux_summary_metric_rows_by_model(results);
    results.ux_summary_metric_rows_by_task_family =
        compute_ux_summary_metric_rows_by_task_family(results);
    results.metrics = Some(compute_eval_metrics(results));
    write_eval_outputs(config, out_path, results)?;
    Ok(())
}

fn write_eval_outputs(
    config: &EvalConfig,
    out_path: &Path,
    results: &EvalResults,
) -> anyhow::Result<()> {
    write_results(out_path, results)?;
    if let Some(junit) = &config.junit {
        write_junit(junit, results)?;
    }
    if let Some(md) = &config.summary_md {
        write_summary_md(md, results)?;
    }
    Ok(())
}

pub(crate) fn push_row(results: &mut EvalResults, row: EvalRunRow) {
    let model = row.model.clone();
    let task_id = row.task_id.clone();
    let model_entry: &mut ModelSummary = results.by_model.entry(model.clone()).or_default();
    let task_entry: &mut TaskSummary = model_entry.tasks.entry(task_id).or_default();
    if row.status == "skipped" {
        model_entry.skipped += 1;
        task_entry.skipped += 1;
    } else if row.passed {
        model_entry.passed += 1;
        task_entry.passed += 1;
    } else {
        model_entry.failed += 1;
        task_entry.failed += 1;
    }
    task_entry.runs.push(row.clone());
    results.runs.push(row);
}

pub(crate) fn finalize_summary(results: &mut EvalResults) {
    results.summary.total_runs = results.runs.len();
    results.summary.passed = results.runs.iter().filter(|r| r.passed).count();
    results.summary.skipped = results
        .runs
        .iter()
        .filter(|r| r.status == "skipped")
        .count();
    results.summary.failed = results
        .summary
        .total_runs
        .saturating_sub(results.summary.passed + results.summary.skipped);
    let denom = results
        .summary
        .total_runs
        .saturating_sub(results.summary.skipped);
    results.summary.pass_rate = if denom == 0 {
        0.0
    } else {
        results.summary.passed as f64 / denom as f64
    };
    for model in results.by_model.values_mut() {
        let total = model.passed + model.failed + model.skipped;
        if total == 0 {
            model.pass_rate = 0.0;
            model.fail_rate = 0.0;
            model.skip_rate = 0.0;
        } else {
            model.pass_rate = model.passed as f64 / total as f64;
            model.fail_rate = model.failed as f64 / total as f64;
            model.skip_rate = model.skipped as f64 / total as f64;
        }
    }
}

fn push_num_metric_row(
    rows: &mut Vec<EvalMetricRow>,
    key: &str,
    value_num: f64,
    direction: EvalMetricDirection,
    is_primary: bool,
) {
    rows.push(EvalMetricRow {
        key: key.to_string(),
        group_name: "ux".to_string(),
        value_num: Some(value_num),
        value_text: None,
        unit: None,
        direction,
        is_primary,
    });
}

fn compute_ux_summary_metric_rows(runs: &[EvalRunRow]) -> Vec<EvalMetricRow> {
    let mut rows = Vec::new();
    let mut validation_required_runs = 0usize;
    let mut validation_passed_runs = 0usize;
    let mut closeout_required_runs = 0usize;
    let mut closeout_passed_runs = 0usize;
    let mut closeout_changed_files_required_runs = 0usize;
    let mut closeout_changed_files_satisfied_runs = 0usize;
    let mut closeout_validation_result_required_runs = 0usize;
    let mut closeout_validation_result_satisfied_runs = 0usize;
    let mut failure_stage_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut task_family_counts = std::collections::BTreeMap::<String, usize>::new();

    for run in runs {
        for metric in &run.ux_metric_rows {
            match metric.key.as_str() {
                "ux.validation_required" if metric.value_num == Some(1.0) => {
                    validation_required_runs = validation_required_runs.saturating_add(1);
                }
                "ux.validation_passed" if metric.value_num == Some(1.0) => {
                    validation_passed_runs = validation_passed_runs.saturating_add(1);
                }
                "ux.exact_closeout_required" if metric.value_num == Some(1.0) => {
                    closeout_required_runs = closeout_required_runs.saturating_add(1);
                }
                "ux.exact_closeout_passed" if metric.value_num == Some(1.0) => {
                    closeout_passed_runs = closeout_passed_runs.saturating_add(1);
                }
                "ux.closeout_changed_files_required" if metric.value_num == Some(1.0) => {
                    closeout_changed_files_required_runs =
                        closeout_changed_files_required_runs.saturating_add(1);
                }
                "ux.closeout_changed_files_satisfied" if metric.value_num == Some(1.0) => {
                    closeout_changed_files_satisfied_runs =
                        closeout_changed_files_satisfied_runs.saturating_add(1);
                }
                "ux.closeout_validation_result_required" if metric.value_num == Some(1.0) => {
                    closeout_validation_result_required_runs =
                        closeout_validation_result_required_runs.saturating_add(1);
                }
                "ux.closeout_validation_result_satisfied" if metric.value_num == Some(1.0) => {
                    closeout_validation_result_satisfied_runs =
                        closeout_validation_result_satisfied_runs.saturating_add(1);
                }
                "ux.failure_stage" => {
                    if let Some(value_text) = &metric.value_text {
                        *failure_stage_counts.entry(value_text.clone()).or_insert(0) += 1;
                    }
                }
                "ux.task_family" => {
                    if let Some(value_text) = &metric.value_text {
                        *task_family_counts.entry(value_text.clone()).or_insert(0) += 1;
                    }
                }
                _ => {}
            }
        }
    }

    let non_skipped_runs = runs.iter().filter(|run| run.status != "skipped").count();
    let passed_non_skipped_runs = runs
        .iter()
        .filter(|run| run.status != "skipped" && run.passed)
        .count();
    let task_success_rate = if non_skipped_runs == 0 {
        0.0
    } else {
        passed_non_skipped_runs as f64 / non_skipped_runs as f64
    };
    let validation_completion_rate = if validation_required_runs == 0 {
        0.0
    } else {
        validation_passed_runs as f64 / validation_required_runs as f64
    };
    let closeout_quality_rate = if closeout_required_runs == 0 {
        0.0
    } else {
        closeout_passed_runs as f64 / closeout_required_runs as f64
    };
    let closeout_changed_files_rate = if closeout_changed_files_required_runs == 0 {
        0.0
    } else {
        closeout_changed_files_satisfied_runs as f64 / closeout_changed_files_required_runs as f64
    };
    let closeout_validation_result_rate = if closeout_validation_result_required_runs == 0 {
        0.0
    } else {
        closeout_validation_result_satisfied_runs as f64
            / closeout_validation_result_required_runs as f64
    };

    push_num_metric_row(
        &mut rows,
        "ux.task_success_rate",
        task_success_rate,
        EvalMetricDirection::HigherIsBetter,
        true,
    );
    push_num_metric_row(
        &mut rows,
        "ux.validation_completion_rate",
        validation_completion_rate,
        EvalMetricDirection::HigherIsBetter,
        true,
    );
    push_num_metric_row(
        &mut rows,
        "ux.closeout_quality_rate",
        closeout_quality_rate,
        EvalMetricDirection::HigherIsBetter,
        true,
    );
    push_num_metric_row(
        &mut rows,
        "ux.closeout_changed_files_rate",
        closeout_changed_files_rate,
        EvalMetricDirection::HigherIsBetter,
        false,
    );
    push_num_metric_row(
        &mut rows,
        "ux.closeout_validation_result_rate",
        closeout_validation_result_rate,
        EvalMetricDirection::HigherIsBetter,
        false,
    );
    push_num_metric_row(
        &mut rows,
        "ux.non_skipped_runs",
        non_skipped_runs as f64,
        EvalMetricDirection::None,
        false,
    );
    push_num_metric_row(
        &mut rows,
        "ux.skipped_runs",
        runs.iter().filter(|run| run.status == "skipped").count() as f64,
        EvalMetricDirection::None,
        false,
    );
    push_num_metric_row(
        &mut rows,
        "ux.validation_required_runs",
        validation_required_runs as f64,
        EvalMetricDirection::None,
        false,
    );
    push_num_metric_row(
        &mut rows,
        "ux.exact_closeout_required_runs",
        closeout_required_runs as f64,
        EvalMetricDirection::None,
        false,
    );
    push_num_metric_row(
        &mut rows,
        "ux.closeout_changed_files_required_runs",
        closeout_changed_files_required_runs as f64,
        EvalMetricDirection::None,
        false,
    );
    push_num_metric_row(
        &mut rows,
        "ux.closeout_validation_result_required_runs",
        closeout_validation_result_required_runs as f64,
        EvalMetricDirection::None,
        false,
    );

    for (stage, count) in failure_stage_counts {
        push_num_metric_row(
            &mut rows,
            &format!("ux.failure_stage.{stage}.count"),
            count as f64,
            EvalMetricDirection::None,
            false,
        );
    }
    for (family, count) in task_family_counts {
        push_num_metric_row(
            &mut rows,
            &format!("ux.task_family.{family}.count"),
            count as f64,
            EvalMetricDirection::None,
            false,
        );
    }

    rows
}

fn compute_ux_summary_metric_rows_by_model(
    results: &EvalResults,
) -> std::collections::BTreeMap<String, Vec<EvalMetricRow>> {
    let mut runs_by_model = std::collections::BTreeMap::<String, Vec<EvalRunRow>>::new();
    for run in &results.runs {
        runs_by_model
            .entry(run.model.clone())
            .or_default()
            .push(run.clone());
    }

    runs_by_model
        .into_iter()
        .map(|(model, runs)| (model, compute_ux_summary_metric_rows(&runs)))
        .collect()
}

fn compute_ux_summary_metric_rows_by_task_family(
    results: &EvalResults,
) -> std::collections::BTreeMap<String, Vec<EvalMetricRow>> {
    let mut runs_by_task_family = std::collections::BTreeMap::<String, Vec<EvalRunRow>>::new();
    for run in &results.runs {
        let task_family = run
            .ux_metric_rows
            .iter()
            .find(|metric| metric.key == "ux.task_family")
            .and_then(|metric| metric.value_text.clone());
        if let Some(task_family) = task_family {
            runs_by_task_family
                .entry(task_family)
                .or_default()
                .push(run.clone());
        }
    }

    runs_by_task_family
        .into_iter()
        .map(|(task_family, runs)| (task_family, compute_ux_summary_metric_rows(&runs)))
        .collect()
}

pub(crate) fn print_row(row: &EvalRunRow) {
    let status = if row.status == "skipped" {
        "SKIP"
    } else if row.passed {
        "PASS"
    } else {
        "FAIL"
    };
    println!(
        "{} | {} | {} | {} | {}",
        row.model, row.task_id, status, row.run_id, row.exit_reason
    );
}

#[cfg(test)]
mod tests {
    use super::{
        compute_ux_summary_metric_rows, compute_ux_summary_metric_rows_by_model,
        compute_ux_summary_metric_rows_by_task_family,
    };
    use crate::eval::types::{
        flatten_ux_metric_rows, EvalFailureStage, EvalMetricRow, EvalResults, EvalResultsConfig,
        EvalRunRow, EvalRunStats, EvalSummary, EvalTaskFamily, EvalUxRunMetrics,
    };

    #[test]
    fn ux_summary_metric_rows_roll_up_primary_rates_and_breakdowns() {
        let run1_ux = EvalUxRunMetrics {
            task_family: Some(EvalTaskFamily::EditWithValidation),
            failure_stage: None,
            validation_required: Some(true),
            validation_attempted: Some(true),
            validation_passed: Some(true),
            exact_closeout_required: Some(true),
            exact_closeout_passed: Some(true),
            closeout_changed_files_required: Some(true),
            closeout_changed_files_satisfied: Some(true),
            closeout_validation_result_required: Some(true),
            closeout_validation_result_satisfied: Some(true),
        };
        let run2_ux = EvalUxRunMetrics {
            task_family: Some(EvalTaskFamily::Recovery),
            failure_stage: Some(EvalFailureStage::Validation),
            validation_required: Some(true),
            validation_attempted: Some(true),
            validation_passed: Some(false),
            exact_closeout_required: Some(true),
            exact_closeout_passed: Some(false),
            closeout_changed_files_required: Some(true),
            closeout_changed_files_satisfied: Some(false),
            closeout_validation_result_required: Some(true),
            closeout_validation_result_satisfied: Some(false),
        };

        let results = EvalResults {
            schema_version: "openagent.eval.v1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            config: EvalResultsConfig::minimal_for_tests(),
            summary: EvalSummary {
                total_runs: 3,
                passed: 1,
                failed: 1,
                skipped: 1,
                pass_rate: 0.5,
            },
            by_model: Default::default(),
            runs: vec![
                EvalRunRow {
                    model: "m".to_string(),
                    task_id: "U5".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r1".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "passed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: true,
                    failures: vec![],
                    stats: EvalRunStats {
                        steps: 4,
                        tool_calls: 2,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: Some(run1_ux.clone()),
                    ux_metric_rows: flatten_ux_metric_rows(&run1_ux),
                },
                EvalRunRow {
                    model: "m".to_string(),
                    task_id: "U6".to_string(),
                    run_index: 1,
                    workdir: None,
                    run_id: "r2".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "failed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: false,
                    failures: vec!["validation failed".to_string()],
                    stats: EvalRunStats {
                        steps: 5,
                        tool_calls: 3,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: Some(run2_ux.clone()),
                    ux_metric_rows: flatten_ux_metric_rows(&run2_ux),
                },
                EvalRunRow {
                    model: "m".to_string(),
                    task_id: "U1".to_string(),
                    run_index: 2,
                    workdir: None,
                    run_id: "r3".to_string(),
                    exit_reason: "skipped".to_string(),
                    status: "skipped".to_string(),
                    skip_reason: Some("missing flags".to_string()),
                    required_flags: vec![],
                    passed: false,
                    failures: vec!["missing flags".to_string()],
                    stats: EvalRunStats {
                        steps: 0,
                        tool_calls: 0,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: None,
                    ux_metric_rows: vec![],
                },
            ],
            ux_summary_metric_rows: vec![],
            ux_summary_metric_rows_by_model: Default::default(),
            ux_summary_metric_rows_by_task_family: Default::default(),
            metrics: None,
            baseline: None,
            regression: None,
        };

        let rows = compute_ux_summary_metric_rows(&results.runs);
        let get = |key: &str| {
            rows.iter()
                .find(|row| row.key == key)
                .and_then(|row| row.value_num)
        };

        assert_eq!(get("ux.task_success_rate"), Some(0.5));
        assert_eq!(get("ux.validation_completion_rate"), Some(0.5));
        assert_eq!(get("ux.closeout_quality_rate"), Some(0.5));
        assert_eq!(get("ux.closeout_changed_files_rate"), Some(0.5));
        assert_eq!(get("ux.closeout_validation_result_rate"), Some(0.5));
        assert_eq!(get("ux.non_skipped_runs"), Some(2.0));
        assert_eq!(get("ux.skipped_runs"), Some(1.0));
        assert_eq!(get("ux.validation_required_runs"), Some(2.0));
        assert_eq!(get("ux.exact_closeout_required_runs"), Some(2.0));
        assert_eq!(get("ux.closeout_changed_files_required_runs"), Some(2.0));
        assert_eq!(
            get("ux.closeout_validation_result_required_runs"),
            Some(2.0)
        );
        assert_eq!(get("ux.failure_stage.validation.count"), Some(1.0));
        assert_eq!(get("ux.task_family.edit_with_validation.count"), Some(1.0));
        assert_eq!(get("ux.task_family.recovery.count"), Some(1.0));
    }

    #[test]
    fn ux_summary_metric_rows_by_model_roll_up_per_model() {
        let pass_ux = EvalUxRunMetrics {
            task_family: Some(EvalTaskFamily::EditWithValidation),
            failure_stage: None,
            validation_required: Some(true),
            validation_attempted: Some(true),
            validation_passed: Some(true),
            exact_closeout_required: Some(true),
            exact_closeout_passed: Some(true),
            closeout_changed_files_required: Some(true),
            closeout_changed_files_satisfied: Some(true),
            closeout_validation_result_required: Some(true),
            closeout_validation_result_satisfied: Some(true),
        };
        let fail_ux = EvalUxRunMetrics {
            task_family: Some(EvalTaskFamily::Recovery),
            failure_stage: Some(EvalFailureStage::Validation),
            validation_required: Some(true),
            validation_attempted: Some(true),
            validation_passed: Some(false),
            exact_closeout_required: Some(true),
            exact_closeout_passed: Some(false),
            closeout_changed_files_required: Some(true),
            closeout_changed_files_satisfied: Some(false),
            closeout_validation_result_required: Some(true),
            closeout_validation_result_satisfied: Some(false),
        };

        let results = EvalResults {
            schema_version: "openagent.eval.v1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            config: EvalResultsConfig::minimal_for_tests(),
            summary: EvalSummary {
                total_runs: 2,
                passed: 1,
                failed: 1,
                skipped: 0,
                pass_rate: 0.5,
            },
            by_model: Default::default(),
            runs: vec![
                EvalRunRow {
                    model: "m_good".to_string(),
                    task_id: "U5".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r1".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "passed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: true,
                    failures: vec![],
                    stats: EvalRunStats {
                        steps: 3,
                        tool_calls: 2,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: Some(pass_ux.clone()),
                    ux_metric_rows: flatten_ux_metric_rows(&pass_ux),
                },
                EvalRunRow {
                    model: "m_bad".to_string(),
                    task_id: "U6".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r2".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "failed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: false,
                    failures: vec!["validation failed".to_string()],
                    stats: EvalRunStats {
                        steps: 4,
                        tool_calls: 3,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: Some(fail_ux.clone()),
                    ux_metric_rows: flatten_ux_metric_rows(&fail_ux),
                },
            ],
            ux_summary_metric_rows: vec![],
            ux_summary_metric_rows_by_model: Default::default(),
            ux_summary_metric_rows_by_task_family: Default::default(),
            metrics: None,
            baseline: None,
            regression: None,
        };

        let by_model = compute_ux_summary_metric_rows_by_model(&results);
        let good = by_model.get("m_good").expect("m_good rows");
        let bad = by_model.get("m_bad").expect("m_bad rows");
        let get = |rows: &Vec<EvalMetricRow>, key: &str| {
            rows.iter()
                .find(|row| row.key == key)
                .and_then(|row| row.value_num)
        };

        assert_eq!(get(good, "ux.task_success_rate"), Some(1.0));
        assert_eq!(get(good, "ux.validation_completion_rate"), Some(1.0));
        assert_eq!(get(bad, "ux.task_success_rate"), Some(0.0));
        assert_eq!(get(bad, "ux.validation_completion_rate"), Some(0.0));
        assert_eq!(get(bad, "ux.failure_stage.validation.count"), Some(1.0));
    }

    #[test]
    fn ux_summary_metric_rows_by_task_family_roll_up_per_family() {
        let fix_pass_ux = EvalUxRunMetrics {
            task_family: Some(EvalTaskFamily::SingleFileFix),
            failure_stage: None,
            validation_required: Some(false),
            validation_attempted: Some(false),
            validation_passed: Some(false),
            exact_closeout_required: Some(false),
            exact_closeout_passed: Some(false),
            closeout_changed_files_required: Some(false),
            closeout_changed_files_satisfied: Some(false),
            closeout_validation_result_required: Some(false),
            closeout_validation_result_satisfied: Some(false),
        };
        let fix_fail_ux = EvalUxRunMetrics {
            task_family: Some(EvalTaskFamily::SingleFileFix),
            failure_stage: Some(EvalFailureStage::Edit),
            validation_required: Some(false),
            validation_attempted: Some(false),
            validation_passed: Some(false),
            exact_closeout_required: Some(false),
            exact_closeout_passed: Some(false),
            closeout_changed_files_required: Some(false),
            closeout_changed_files_satisfied: Some(false),
            closeout_validation_result_required: Some(false),
            closeout_validation_result_satisfied: Some(false),
        };
        let recovery_ux = EvalUxRunMetrics {
            task_family: Some(EvalTaskFamily::Recovery),
            failure_stage: Some(EvalFailureStage::Validation),
            validation_required: Some(true),
            validation_attempted: Some(true),
            validation_passed: Some(false),
            exact_closeout_required: Some(true),
            exact_closeout_passed: Some(false),
            closeout_changed_files_required: Some(false),
            closeout_changed_files_satisfied: Some(false),
            closeout_validation_result_required: Some(false),
            closeout_validation_result_satisfied: Some(false),
        };

        let results = EvalResults {
            schema_version: "openagent.eval.v1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            config: EvalResultsConfig::minimal_for_tests(),
            summary: EvalSummary {
                total_runs: 3,
                passed: 1,
                failed: 2,
                skipped: 0,
                pass_rate: 1.0 / 3.0,
            },
            by_model: Default::default(),
            runs: vec![
                EvalRunRow {
                    model: "m1".to_string(),
                    task_id: "U3".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r1".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "passed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: true,
                    failures: vec![],
                    stats: EvalRunStats {
                        steps: 2,
                        tool_calls: 1,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: Some(fix_pass_ux.clone()),
                    ux_metric_rows: flatten_ux_metric_rows(&fix_pass_ux),
                },
                EvalRunRow {
                    model: "m2".to_string(),
                    task_id: "U3b".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r2".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "failed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: false,
                    failures: vec!["edit failed".to_string()],
                    stats: EvalRunStats {
                        steps: 3,
                        tool_calls: 2,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: Some(fix_fail_ux.clone()),
                    ux_metric_rows: flatten_ux_metric_rows(&fix_fail_ux),
                },
                EvalRunRow {
                    model: "m3".to_string(),
                    task_id: "U6".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r3".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "failed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: false,
                    failures: vec!["validation failed".to_string()],
                    stats: EvalRunStats {
                        steps: 4,
                        tool_calls: 3,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: None,
                    ux: Some(recovery_ux.clone()),
                    ux_metric_rows: flatten_ux_metric_rows(&recovery_ux),
                },
            ],
            ux_summary_metric_rows: vec![],
            ux_summary_metric_rows_by_model: Default::default(),
            ux_summary_metric_rows_by_task_family: Default::default(),
            metrics: None,
            baseline: None,
            regression: None,
        };

        let by_task_family = compute_ux_summary_metric_rows_by_task_family(&results);
        let single_file_fix = by_task_family
            .get("single_file_fix")
            .expect("single_file_fix rows");
        let recovery = by_task_family.get("recovery").expect("recovery rows");
        let get = |rows: &Vec<EvalMetricRow>, key: &str| {
            rows.iter()
                .find(|row| row.key == key)
                .and_then(|row| row.value_num)
        };

        assert_eq!(get(single_file_fix, "ux.task_success_rate"), Some(0.5));
        assert_eq!(
            get(single_file_fix, "ux.failure_stage.edit.count"),
            Some(1.0)
        );
        assert_eq!(
            get(single_file_fix, "ux.task_family.single_file_fix.count"),
            Some(2.0)
        );
        assert_eq!(get(recovery, "ux.task_success_rate"), Some(0.0));
        assert_eq!(get(recovery, "ux.validation_completion_rate"), Some(0.0));
        assert_eq!(
            get(recovery, "ux.failure_stage.validation.count"),
            Some(1.0)
        );
    }
}
