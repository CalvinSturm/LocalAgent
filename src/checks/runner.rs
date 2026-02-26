use std::path::{Path, PathBuf};

use crate::checks::loader::{load_checks, CheckLoadError, LoadedCheck};
use crate::checks::report::{CheckRunReport, CheckRunResult};
use crate::checks::schema::PassCriteriaType;

#[derive(Debug, Clone)]
pub struct CheckRunArgs {
    pub path: Option<PathBuf>,
    pub max_checks: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckRunExit {
    Ok = 0,
    InvalidChecks = 2,
    FailedChecks = 3,
    RunnerError = 4,
}

pub fn load_checks_for_run(
    root: &Path,
    args: &CheckRunArgs,
) -> Result<Vec<LoadedCheck>, Box<(CheckRunReport, CheckRunExit)>> {
    let loaded = load_checks(root, args.path.as_deref());
    if !loaded.errors.is_empty() {
        return Err(Box::new((
            report_from_loader_errors(loaded.errors),
            CheckRunExit::InvalidChecks,
        )));
    }
    let mut checks = loaded.checks;
    if let Some(max) = args.max_checks {
        checks.truncate(max);
    }
    Ok(checks)
}

pub fn report_from_loader_errors(errors: Vec<CheckLoadError>) -> CheckRunReport {
    let results = errors
        .into_iter()
        .map(|e| CheckRunResult {
            description: None,
            name: e.path.clone().unwrap_or_else(|| "loader".to_string()),
            path: e.path.unwrap_or_else(|| ".".to_string()),
            status: "error".to_string(),
            reason_code: Some(e.code),
            summary: e.message,
            required: false,
            file_bytes_hash_hex: String::new(),
            frontmatter_hash_hex: String::new(),
            check_hash_hex: String::new(),
        })
        .collect::<Vec<_>>();
    CheckRunReport::from_results(results)
}

pub fn report_single_error(code: &str, message: impl Into<String>) -> CheckRunReport {
    CheckRunReport::from_results(vec![CheckRunResult {
        name: "runner".to_string(),
        path: ".".to_string(),
        description: None,
        status: "error".to_string(),
        reason_code: Some(code.to_string()),
        summary: message.into(),
        required: false,
        file_bytes_hash_hex: String::new(),
        frontmatter_hash_hex: String::new(),
        check_hash_hex: String::new(),
    }])
}

pub fn evaluate_final_output(check: &LoadedCheck, final_output: &str) -> Result<(), String> {
    let value = &check.frontmatter.pass_criteria.value;
    match check.frontmatter.pass_criteria.kind {
        PassCriteriaType::Contains => {
            if final_output.contains(value) {
                Ok(())
            } else {
                Err(format!(
                    "final_output missing expected substring: {}",
                    value
                ))
            }
        }
        PassCriteriaType::NotContains => {
            if final_output.contains(value) {
                Err(format!(
                    "final_output contains forbidden substring: {}",
                    value
                ))
            } else {
                Ok(())
            }
        }
        PassCriteriaType::Equals => {
            if final_output == value {
                Ok(())
            } else {
                Err("final_output did not equal expected value".to_string())
            }
        }
    }
}
