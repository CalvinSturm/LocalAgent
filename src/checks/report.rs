use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct CheckRunResult {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub status: String,
    pub reason_code: Option<String>,
    pub summary: String,
    pub required: bool,
    pub file_bytes_hash_hex: String,
    pub frontmatter_hash_hex: String,
    pub check_hash_hex: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckRunReport {
    pub schema_version: String,
    pub checks: Vec<CheckRunResult>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl CheckRunReport {
    pub fn from_results(checks: Vec<CheckRunResult>) -> Self {
        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut errors = 0;
        for c in &checks {
            match c.status.as_str() {
                "passed" => passed += 1,
                "failed" => failed += 1,
                "skipped" => skipped += 1,
                _ => errors += 1,
            }
        }
        Self {
            schema_version: "localagent.checks.report.v1".to_string(),
            checks,
            passed,
            failed,
            skipped,
            errors,
        }
    }
}

pub fn write_junit(path: &Path, report: &CheckRunReport) -> anyhow::Result<()> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
    xml.push_str(&format!(
        "<testsuite name=\"localagent-checks\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"{}\">\n",
        report.checks.len(),
        report.failed,
        report.skipped,
        report.errors
    ));
    for c in &report.checks {
        xml.push_str(&format!(
            "<testcase classname=\"{}\" name=\"{}\">",
            xml_escape(&c.path),
            xml_escape(&c.name)
        ));
        match c.status.as_str() {
            "failed" => xml.push_str(&format!(
                "<failure message=\"{}\">{}</failure>",
                xml_escape(c.reason_code.as_deref().unwrap_or("CHECK_FAIL")),
                xml_escape(&c.summary)
            )),
            "skipped" => xml.push_str(&format!(
                "<skipped message=\"{}\"/>",
                xml_escape(c.reason_code.as_deref().unwrap_or("CHECK_SKIPPED"))
            )),
            "error" => xml.push_str(&format!(
                "<error message=\"{}\">{}</error>",
                xml_escape(c.reason_code.as_deref().unwrap_or("CHECK_ERROR")),
                xml_escape(&c.summary)
            )),
            _ => {}
        }
        xml.push_str("</testcase>\n");
    }
    xml.push_str("</testsuite>\n</testsuites>\n");
    std::fs::write(path, xml)?;
    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
