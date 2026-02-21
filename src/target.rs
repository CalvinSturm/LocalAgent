use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use clap::ValueEnum;
use serde::Serialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ExecTargetKind {
    Host,
    Docker,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerMeta {
    pub image: String,
    pub workdir: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetDescribe {
    pub exec_target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerMeta>,
}

#[derive(Debug, Clone)]
pub struct TargetResult {
    pub ok: bool,
    pub content: String,
    pub truncated: bool,
    pub bytes: Option<u64>,
    pub exit_code: Option<i32>,
    pub stderr_truncated: Option<bool>,
    pub stdout_truncated: Option<bool>,
    pub execution_target: ExecTargetKind,
    pub docker: Option<DockerMeta>,
}

impl TargetResult {
    pub fn failed(kind: ExecTargetKind, reason: String, docker: Option<DockerMeta>) -> Self {
        Self {
            ok: false,
            content: reason,
            truncated: false,
            bytes: None,
            exit_code: None,
            stderr_truncated: None,
            stdout_truncated: None,
            execution_target: kind,
            docker,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellReq {
    pub workdir: PathBuf,
    pub cmd: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub max_tool_output_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct ReadReq {
    pub workdir: PathBuf,
    pub path: String,
    pub max_read_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct ListReq {
    pub workdir: PathBuf,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct WriteReq {
    pub workdir: PathBuf,
    pub path: String,
    pub content: String,
    pub create_parents: bool,
}

#[derive(Debug, Clone)]
pub struct PatchReq {
    pub workdir: PathBuf,
    pub path: String,
    pub patch: String,
}

#[async_trait]
pub trait ExecTarget: Send + Sync {
    fn kind(&self) -> ExecTargetKind;
    fn describe(&self) -> TargetDescribe;
    async fn exec_shell(&self, req: ShellReq) -> TargetResult;
    async fn read_file(&self, req: ReadReq) -> TargetResult;
    async fn list_dir(&self, req: ListReq) -> TargetResult;
    async fn write_file(&self, req: WriteReq) -> TargetResult;
    async fn apply_patch(&self, req: PatchReq) -> TargetResult;
}

#[derive(Debug, Clone, Default)]
pub struct HostTarget;

#[async_trait]
impl ExecTarget for HostTarget {
    fn kind(&self) -> ExecTargetKind {
        ExecTargetKind::Host
    }

    fn describe(&self) -> TargetDescribe {
        TargetDescribe {
            exec_target: "host".to_string(),
            docker: None,
        }
    }

    async fn exec_shell(&self, req: ShellReq) -> TargetResult {
        let mut command = Command::new(&req.cmd);
        for a in &req.args {
            command.arg(a);
        }
        let cwd = req
            .cwd
            .as_deref()
            .map(|c| resolve_path(&req.workdir, c))
            .unwrap_or_else(|| req.workdir.clone());
        command.current_dir(cwd);
        match command.output().await {
            Ok(output) => {
                let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
                let (stdout, stdout_truncated) =
                    truncate_utf8_to_bytes(&stdout_raw, req.max_tool_output_bytes);
                let (stderr, stderr_truncated) =
                    truncate_utf8_to_bytes(&stderr_raw, req.max_tool_output_bytes);
                TargetResult {
                    ok: output.status.success(),
                    content: json!({
                        "status": output.status.code(),
                        "stdout": stdout,
                        "stderr": stderr,
                        "stdout_truncated": stdout_truncated,
                        "stderr_truncated": stderr_truncated,
                        "max_tool_output_bytes": req.max_tool_output_bytes
                    })
                    .to_string(),
                    truncated: stdout_truncated || stderr_truncated,
                    bytes: Some((output.stdout.len() + output.stderr.len()) as u64),
                    exit_code: output.status.code(),
                    stderr_truncated: Some(stderr_truncated),
                    stdout_truncated: Some(stdout_truncated),
                    execution_target: ExecTargetKind::Host,
                    docker: None,
                }
            }
            Err(e) => TargetResult::failed(
                ExecTargetKind::Host,
                format!("shell execution failed: {e}"),
                None,
            ),
        }
    }

    async fn read_file(&self, req: ReadReq) -> TargetResult {
        let full = resolve_path(&req.workdir, &req.path);
        match tokio::fs::read(&full).await {
            Ok(bytes) => {
                let raw = String::from_utf8_lossy(&bytes).to_string();
                let (content, truncated) = truncate_utf8_to_bytes(&raw, req.max_read_bytes);
                TargetResult {
                    ok: true,
                    content: json!({
                        "path": full.display().to_string(),
                        "content": content,
                        "truncated": truncated,
                        "max_read_bytes": req.max_read_bytes,
                        "read_bytes": bytes.len()
                    })
                    .to_string(),
                    truncated,
                    bytes: Some(bytes.len() as u64),
                    exit_code: None,
                    stderr_truncated: None,
                    stdout_truncated: None,
                    execution_target: ExecTargetKind::Host,
                    docker: None,
                }
            }
            Err(e) => TargetResult::failed(
                ExecTargetKind::Host,
                format!("read_file failed for {}: {e}", full.display()),
                None,
            ),
        }
    }

    async fn list_dir(&self, req: ListReq) -> TargetResult {
        let full = resolve_path(&req.workdir, &req.path);
        let mut entries = Vec::new();
        match tokio::fs::read_dir(&full).await {
            Ok(mut rd) => loop {
                match rd.next_entry().await {
                    Ok(Some(entry)) => {
                        let file_name = entry.file_name().to_string_lossy().to_string();
                        match entry.metadata().await {
                            Ok(meta) => entries.push(
                                json!({"name":file_name,"is_dir":meta.is_dir(),"len":meta.len()}),
                            ),
                            Err(e) => entries.push(json!({"name":file_name,"error":e.to_string()})),
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        return TargetResult::failed(
                            ExecTargetKind::Host,
                            format!("list_dir failed for {}: {e}", full.display()),
                            None,
                        )
                    }
                }
            },
            Err(e) => {
                return TargetResult::failed(
                    ExecTargetKind::Host,
                    format!("list_dir failed for {}: {e}", full.display()),
                    None,
                )
            }
        }
        TargetResult {
            ok: true,
            content: json!({"path":full.display().to_string(),"entries":entries}).to_string(),
            truncated: false,
            bytes: None,
            exit_code: None,
            stderr_truncated: None,
            stdout_truncated: None,
            execution_target: ExecTargetKind::Host,
            docker: None,
        }
    }

    async fn write_file(&self, req: WriteReq) -> TargetResult {
        let full = resolve_path(&req.workdir, &req.path);
        if req.create_parents {
            if let Some(parent) = full.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return TargetResult::failed(
                        ExecTargetKind::Host,
                        format!("write_file failed for {}: {e}", full.display()),
                        None,
                    );
                }
            }
        }
        match tokio::fs::write(&full, req.content.as_bytes()).await {
            Ok(()) => TargetResult {
                ok: true,
                content:
                    json!({"path":full.display().to_string(),"bytes_written":req.content.len()})
                        .to_string(),
                truncated: false,
                bytes: Some(req.content.len() as u64),
                exit_code: None,
                stderr_truncated: None,
                stdout_truncated: None,
                execution_target: ExecTargetKind::Host,
                docker: None,
            },
            Err(e) => TargetResult::failed(
                ExecTargetKind::Host,
                format!("write_file failed for {}: {e}", full.display()),
                None,
            ),
        }
    }

    async fn apply_patch(&self, req: PatchReq) -> TargetResult {
        let full = resolve_path(&req.workdir, &req.path);
        let original_bytes = match tokio::fs::read(&full).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return TargetResult::failed(
                    ExecTargetKind::Host,
                    format!("apply_patch failed for {}: {e}", full.display()),
                    None,
                )
            }
        };
        let original = String::from_utf8_lossy(&original_bytes).to_string();
        let patch = match diffy::Patch::from_str(&req.patch) {
            Ok(p) => p,
            Err(e) => {
                return TargetResult::failed(
                    ExecTargetKind::Host,
                    format!("invalid patch: {e}"),
                    None,
                )
            }
        };
        let patched = match diffy::apply(&original, &patch) {
            Ok(p) => p,
            Err(e) => {
                return TargetResult::failed(
                    ExecTargetKind::Host,
                    format!("failed to apply patch: {e}"),
                    None,
                )
            }
        };
        if let Some(parent) = full.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return TargetResult::failed(
                    ExecTargetKind::Host,
                    format!("apply_patch failed for {}: {e}", full.display()),
                    None,
                );
            }
        }
        match tokio::fs::write(&full, patched.as_bytes()).await {
            Ok(()) => TargetResult {
                ok: true,
                content: json!({"path":full.display().to_string(),"changed":patched!=original,"bytes_written":patched.len()}).to_string(),
                truncated: false,
                bytes: Some(patched.len() as u64),
                exit_code: None,
                stderr_truncated: None,
                stdout_truncated: None,
                execution_target: ExecTargetKind::Host,
                docker: None,
            },
            Err(e) => TargetResult::failed(
                ExecTargetKind::Host,
                format!("apply_patch failed for {}: {e}", full.display()),
                None,
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DockerTarget {
    meta: DockerMeta,
}

impl DockerTarget {
    pub fn new(image: String, workdir: String, network: String, user: Option<String>) -> Self {
        Self {
            meta: DockerMeta {
                image,
                workdir,
                network,
                user,
            },
        }
    }

    pub fn validate_available() -> anyhow::Result<()> {
        let out = std::process::Command::new("docker")
            .arg("version")
            .arg("--format")
            .arg("{{.Server.Version}}")
            .output()
            .context("failed to execute `docker version`")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!(
                "docker execution target requested but Docker is unavailable: {}",
                stderr.trim()
            ));
        }
        Ok(())
    }

    async fn run_container(
        &self,
        host_workdir: &Path,
        shell_script: &str,
        stdin_bytes: Option<&[u8]>,
        max_tool_output_bytes: usize,
    ) -> TargetResult {
        let mount = format!("{}:{}", host_workdir.display(), self.meta.workdir);
        let mut cmd = Command::new("docker");
        cmd.arg("run").arg("--rm");
        if self.meta.network == "none" {
            cmd.arg("--network").arg("none");
        } else {
            cmd.arg("--network").arg("bridge");
        }
        if let Some(user) = &self.meta.user {
            cmd.arg("--user").arg(user);
        }
        cmd.arg("-v")
            .arg(mount)
            .arg("-w")
            .arg(&self.meta.workdir)
            .arg(&self.meta.image)
            .arg("sh")
            .arg("-lc")
            .arg(shell_script)
            .stdin(if stdin_bytes.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(data) = stdin_bytes {
                    if let Some(mut stdin) = child.stdin.take() {
                        if let Err(e) = stdin.write_all(data).await {
                            return TargetResult::failed(
                                ExecTargetKind::Docker,
                                format!("docker stdin write failed: {e}"),
                                Some(self.meta.clone()),
                            );
                        }
                    }
                }
                match child.wait_with_output().await {
                    Ok(output) => {
                        let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
                        let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
                        let (stdout, stdout_truncated) =
                            truncate_utf8_to_bytes(&stdout_raw, max_tool_output_bytes);
                        let (stderr, stderr_truncated) =
                            truncate_utf8_to_bytes(&stderr_raw, max_tool_output_bytes);
                        TargetResult {
                            ok: output.status.success(),
                            content: json!({
                                "status": output.status.code(),
                                "stdout": stdout,
                                "stderr": stderr,
                                "stdout_truncated": stdout_truncated,
                                "stderr_truncated": stderr_truncated,
                                "max_tool_output_bytes": max_tool_output_bytes
                            })
                            .to_string(),
                            truncated: stdout_truncated || stderr_truncated,
                            bytes: Some((output.stdout.len() + output.stderr.len()) as u64),
                            exit_code: output.status.code(),
                            stderr_truncated: Some(stderr_truncated),
                            stdout_truncated: Some(stdout_truncated),
                            execution_target: ExecTargetKind::Docker,
                            docker: Some(self.meta.clone()),
                        }
                    }
                    Err(e) => TargetResult::failed(
                        ExecTargetKind::Docker,
                        format!("docker command failed: {e}"),
                        Some(self.meta.clone()),
                    ),
                }
            }
            Err(e) => TargetResult::failed(
                ExecTargetKind::Docker,
                format!("failed to spawn docker: {e}"),
                Some(self.meta.clone()),
            ),
        }
    }
}

#[async_trait]
impl ExecTarget for DockerTarget {
    fn kind(&self) -> ExecTargetKind {
        ExecTargetKind::Docker
    }

    fn describe(&self) -> TargetDescribe {
        TargetDescribe {
            exec_target: "docker".to_string(),
            docker: Some(self.meta.clone()),
        }
    }

    async fn exec_shell(&self, req: ShellReq) -> TargetResult {
        let args = req
            .args
            .iter()
            .map(|a| shell_escape(a))
            .collect::<Vec<_>>()
            .join(" ");
        let cwd = req.cwd.unwrap_or_else(|| ".".to_string());
        let script = format!(
            "cd {} && {} {}",
            shell_escape(&cwd),
            shell_escape(&req.cmd),
            args
        );
        self.run_container(&req.workdir, &script, None, req.max_tool_output_bytes)
            .await
    }

    async fn read_file(&self, req: ReadReq) -> TargetResult {
        let script = format!("cat -- {}", shell_escape(&req.path));
        let mut out = self
            .run_container(&req.workdir, &script, None, req.max_read_bytes)
            .await;
        if out.ok {
            let parsed: serde_json::Value = match serde_json::from_str(&out.content) {
                Ok(v) => v,
                Err(_) => {
                    return TargetResult::failed(
                        ExecTargetKind::Docker,
                        "failed to parse docker read output".to_string(),
                        Some(self.meta.clone()),
                    )
                }
            };
            let stdout = parsed
                .get("stdout")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let (content, truncated) = truncate_utf8_to_bytes(&stdout, req.max_read_bytes);
            out.content = json!({
                "path": req.path,
                "content": content,
                "truncated": truncated,
                "max_read_bytes": req.max_read_bytes,
                "read_bytes": stdout.len()
            })
            .to_string();
            out.truncated = truncated;
            out.bytes = Some(stdout.len() as u64);
        }
        out
    }

    async fn list_dir(&self, req: ListReq) -> TargetResult {
        let script = format!(
            "for p in {}/*; do [ -e \"$p\" ] || continue; n=$(basename \"$p\"); if [ -d \"$p\" ]; then d=true; else d=false; fi; l=$(wc -c < \"$p\" 2>/dev/null || echo 0); printf '%s\\t%s\\t%s\\n' \"$n\" \"$d\" \"$l\"; done",
            shell_escape(&req.path)
        );
        let mut out = self
            .run_container(&req.workdir, &script, None, 200_000)
            .await;
        if out.ok {
            let parsed: serde_json::Value = match serde_json::from_str(&out.content) {
                Ok(v) => v,
                Err(_) => {
                    return TargetResult::failed(
                        ExecTargetKind::Docker,
                        "failed to parse docker list output".to_string(),
                        Some(self.meta.clone()),
                    )
                }
            };
            let stdout = parsed
                .get("stdout")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let entries = stdout
                .lines()
                .filter_map(|line| {
                    let parts = line.split('\t').collect::<Vec<_>>();
                    if parts.len() < 3 {
                        return None;
                    }
                    Some(json!({
                        "name": parts[0],
                        "is_dir": parts[1] == "true",
                        "len": parts[2].parse::<u64>().unwrap_or(0)
                    }))
                })
                .collect::<Vec<_>>();
            out.content = json!({"path": req.path, "entries": entries}).to_string();
            out.truncated = false;
        }
        out
    }

    async fn write_file(&self, req: WriteReq) -> TargetResult {
        let prep = if req.create_parents {
            format!(
                "mkdir -p $(dirname -- {}) && cat > {}",
                shell_escape(&req.path),
                shell_escape(&req.path)
            )
        } else {
            format!("cat > {}", shell_escape(&req.path))
        };
        let mut out = self
            .run_container(&req.workdir, &prep, Some(req.content.as_bytes()), 200_000)
            .await;
        if out.ok {
            out.content = json!({"path": req.path, "bytes_written": req.content.len()}).to_string();
            out.bytes = Some(req.content.len() as u64);
            out.truncated = false;
        }
        out
    }

    async fn apply_patch(&self, req: PatchReq) -> TargetResult {
        let script = format!(
            "patch -u {} <<'OPENAGENT_PATCH'\n{}\nOPENAGENT_PATCH",
            shell_escape(&req.path),
            req.patch
        );
        let mut out = self
            .run_container(&req.workdir, &script, None, 200_000)
            .await;
        if out.ok {
            out.content =
                json!({"path": req.path, "changed": true, "bytes_written": 0}).to_string();
            out.truncated = false;
        }
        out
    }
}

pub fn resolve_path(workdir: &Path, input: &str) -> PathBuf {
    let p = PathBuf::from(input);
    if p.is_absolute() {
        p
    } else {
        workdir.join(p)
    }
}

fn truncate_utf8_to_bytes(input: &str, max_bytes: usize) -> (String, bool) {
    if max_bytes == 0 {
        return (input.to_string(), false);
    }
    if input.len() <= max_bytes {
        return (input.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (input[..end].to_string(), true)
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::ExecTargetKind;
    use clap::ValueEnum;

    #[test]
    fn exec_target_kind_parse() {
        assert!(ExecTargetKind::from_str("host", true).is_ok());
        assert!(ExecTargetKind::from_str("docker", true).is_ok());
    }
}
