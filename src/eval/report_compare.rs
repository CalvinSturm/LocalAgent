use std::collections::BTreeMap;
use std::path::Path;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::eval::runner::{EvalAggregateMetrics, EvalResults};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareReport {
    pub schema_version: String,
    pub summary_delta: MetricDelta,
    pub per_model: BTreeMap<String, MetricDelta>,
    pub top_task_regressions: Vec<TaskRegression>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricDelta {
    pub pass_rate_delta: f64,
    pub avg_steps_delta: f64,
    pub avg_tool_calls_delta: f64,
    pub avg_tool_retries_delta: f64,
    pub avg_wall_time_ms_delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRegression {
    pub task_id: String,
    pub pass_rate_delta: f64,
    pub avg_steps_delta: f64,
}

pub fn compare_results_files(
    a: &Path,
    b: &Path,
    out_md: &Path,
    out_json: Option<&Path>,
) -> anyhow::Result<()> {
    let ra: EvalResults = serde_json::from_slice(&std::fs::read(a)?)?;
    let rb: EvalResults = serde_json::from_slice(&std::fs::read(b)?)?;
    if ra.schema_version != "openagent.eval.v1" || rb.schema_version != "openagent.eval.v1" {
        return Err(anyhow!(
            "schema mismatch: expected openagent.eval.v1 in both inputs"
        ));
    }
    let rep = build_compare_report(&ra, &rb);
    if let Some(parent) = out_md.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_md, render_markdown(&rep))?;
    if let Some(p) = out_json {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, serde_json::to_vec_pretty(&rep)?)?;
    }
    Ok(())
}

pub fn build_compare_report(a: &EvalResults, b: &EvalResults) -> CompareReport {
    let a_metrics = a.metrics.clone().unwrap_or_default();
    let b_metrics = b.metrics.clone().unwrap_or_default();
    let mut per_model = BTreeMap::new();
    let mut models = a_metrics
        .per_model
        .keys()
        .chain(b_metrics.per_model.keys())
        .cloned()
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    for m in models {
        let am = a_metrics.per_model.get(&m).cloned().unwrap_or_default();
        let bm = b_metrics.per_model.get(&m).cloned().unwrap_or_default();
        per_model.insert(m, delta(&am, &bm));
    }

    let mut task_regs = Vec::new();
    let mut tasks = a_metrics
        .per_task
        .keys()
        .chain(b_metrics.per_task.keys())
        .cloned()
        .collect::<Vec<_>>();
    tasks.sort();
    tasks.dedup();
    for t in tasks {
        let at = a_metrics.per_task.get(&t).cloned().unwrap_or_default();
        let bt = b_metrics.per_task.get(&t).cloned().unwrap_or_default();
        task_regs.push(TaskRegression {
            task_id: t,
            pass_rate_delta: bt.pass_rate - at.pass_rate,
            avg_steps_delta: bt.avg_steps - at.avg_steps,
        });
    }
    task_regs.sort_by(|x, y| {
        x.pass_rate_delta
            .partial_cmp(&y.pass_rate_delta)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                y.avg_steps_delta
                    .partial_cmp(&x.avg_steps_delta)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    task_regs.truncate(10);

    CompareReport {
        schema_version: "openagent.eval_compare.v1".to_string(),
        summary_delta: delta(&a_metrics.summary, &b_metrics.summary),
        per_model,
        top_task_regressions: task_regs,
    }
}

fn delta(a: &EvalAggregateMetrics, b: &EvalAggregateMetrics) -> MetricDelta {
    MetricDelta {
        pass_rate_delta: b.pass_rate - a.pass_rate,
        avg_steps_delta: b.avg_steps - a.avg_steps,
        avg_tool_calls_delta: b.avg_tool_calls - a.avg_tool_calls,
        avg_tool_retries_delta: b.avg_tool_retries - a.avg_tool_retries,
        avg_wall_time_ms_delta: b.avg_wall_time_ms - a.avg_wall_time_ms,
    }
}

fn render_markdown(rep: &CompareReport) -> String {
    let mut md = String::new();
    md.push_str("# Eval Compare Report\n\n");
    md.push_str("## Summary delta (B - A)\n\n");
    md.push_str(&format!(
        "- pass_rate: {:+.4}\n- avg_steps: {:+.4}\n- avg_tool_calls: {:+.4}\n- avg_tool_retries: {:+.4}\n- avg_wall_time_ms: {:+.4}\n\n",
        rep.summary_delta.pass_rate_delta,
        rep.summary_delta.avg_steps_delta,
        rep.summary_delta.avg_tool_calls_delta,
        rep.summary_delta.avg_tool_retries_delta,
        rep.summary_delta.avg_wall_time_ms_delta
    ));
    md.push_str("## Per model\n\n");
    for (model, d) in &rep.per_model {
        md.push_str(&format!(
            "- {}: pass_rate {:+.4}, avg_steps {:+.4}, avg_tool_calls {:+.4}, avg_tool_retries {:+.4}, avg_wall_time_ms {:+.4}\n",
            model, d.pass_rate_delta, d.avg_steps_delta, d.avg_tool_calls_delta, d.avg_tool_retries_delta, d.avg_wall_time_ms_delta
        ));
    }
    md.push_str("\n## Top task regressions\n\n");
    for r in &rep.top_task_regressions {
        md.push_str(&format!(
            "- {}: pass_rate {:+.4}, avg_steps {:+.4}\n",
            r.task_id, r.pass_rate_delta, r.avg_steps_delta
        ));
    }
    md
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::eval::runner::{
        EvalAggregateMetrics, EvalMetrics, EvalResults, EvalResultsConfig, EvalSummary,
    };

    use super::build_compare_report;

    #[test]
    fn compare_report_contains_expected_deltas() {
        let a = EvalResults {
            schema_version: "openagent.eval.v1".to_string(),
            created_at: "x".to_string(),
            config: EvalResultsConfig::minimal_for_tests(),
            summary: EvalSummary::default(),
            by_model: BTreeMap::new(),
            runs: Vec::new(),
            metrics: Some(EvalMetrics {
                summary: EvalAggregateMetrics {
                    pass_rate: 0.8,
                    avg_steps: 10.0,
                    avg_tool_calls: 2.0,
                    avg_wall_time_ms: 1000.0,
                    ..Default::default()
                },
                per_model: BTreeMap::new(),
                per_task: BTreeMap::new(),
            }),
            baseline: None,
            regression: None,
        };
        let mut b = a.clone();
        b.metrics = Some(EvalMetrics {
            summary: EvalAggregateMetrics {
                pass_rate: 0.7,
                avg_steps: 12.0,
                avg_tool_calls: 3.0,
                avg_wall_time_ms: 1100.0,
                ..Default::default()
            },
            per_model: BTreeMap::new(),
            per_task: BTreeMap::new(),
        });
        let rep = build_compare_report(&a, &b);
        assert!(rep.summary_delta.pass_rate_delta < 0.0);
        assert!(rep.summary_delta.avg_steps_delta > 0.0);
    }
}
