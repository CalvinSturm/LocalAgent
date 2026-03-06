use std::path::Path;

use crate::eval::metrics::compute_eval_metrics;
use crate::eval::report::{write_junit, write_results, write_summary_md};
use crate::eval::types::{EvalConfig, EvalResults, EvalRunRow, ModelSummary, TaskSummary};

pub(crate) fn finalize_and_write_eval_results(
    config: &EvalConfig,
    out_path: &Path,
    results: &mut EvalResults,
) -> anyhow::Result<()> {
    finalize_summary(results);
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
