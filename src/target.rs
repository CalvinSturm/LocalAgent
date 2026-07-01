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

/// Which standard stream a live output chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellStreamKind {
    Stdout,
    Stderr,
}

impl ShellStreamKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ShellStreamKind::Stdout => "stdout",
            ShellStreamKind::Stderr => "stderr",
        }
    }
}

/// An incremental slice of shell output produced while the command is still
/// running. Emitted for live progress display only; the final result envelope
/// is unaffected and remains the source of truth.
#[derive(Debug, Clone)]
pub struct ShellOutputChunk {
    pub stream: ShellStreamKind,
    pub bytes: Vec<u8>,
}

/// Sender used to forward live shell output chunks to a consumer (e.g. the TUI
/// tail). Cloneable and bounded; producers use best-effort non-blocking sends
/// so noisy output cannot backpressure or grow memory without bound.
pub type ShellOutputTx = tokio::sync::mpsc::Sender<ShellOutputChunk>;

#[derive(Debug, Clone)]
pub struct ShellReq {
    pub workdir: PathBuf,
    pub cmd: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub max_tool_output_bytes: usize,
    /// Wall-clock timeout for the command, in milliseconds. `0` means unbounded
    /// (the historical default). The `u64` type makes negative values
    /// unrepresentable. Timeout enforcement currently applies to the host
    /// execution target only; the docker target ignores this field (follow-up).
    pub timeout_ms: u64,
    /// Optional sink for live output chunks while the command runs. `None`
    /// disables streaming (unchanged behavior). Honored by the host target only;
    /// the docker target ignores it (follow-up), and the final result envelope
    /// is identical either way.
    pub stream: Option<ShellOutputTx>,
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
        let cwd =
            match req.cwd.as_deref() {
                Some(cwd_str) => match resolve_path_scoped(&req.workdir, cwd_str) {
                    Ok(path) => path,
                    Err(_) => return TargetResult::failed(
                        ExecTargetKind::Host,
                        "shell cwd must stay within workdir (no absolute paths or '..' traversal)"
                            .to_string(),
                        None,
                    ),
                },
                None => req.workdir.clone(),
            };
        command.current_dir(cwd);
        match spawn_and_wait_managed(command, req.timeout_ms, None, req.stream.clone()).await {
            Ok(managed) => build_shell_target_result(
                ExecTargetKind::Host,
                None,
                managed,
                req.timeout_ms,
                req.max_tool_output_bytes,
            ),
            Err(e) => TargetResult::failed(
                ExecTargetKind::Host,
                format!("shell execution failed: {e}"),
                None,
            ),
        }
    }

    async fn read_file(&self, req: ReadReq) -> TargetResult {
        let full =
            match resolve_path_scoped(&req.workdir, &req.path) {
                Ok(path) => path,
                Err(_) => return TargetResult::failed(
                    ExecTargetKind::Host,
                    "read_file path must stay within workdir (no absolute paths or '..' traversal)"
                        .to_string(),
                    None,
                ),
            };
        match tokio::fs::read(&full).await {
            Ok(bytes) => {
                let raw = String::from_utf8_lossy(&bytes).to_string();
                let (content, truncated) = truncate_utf8_to_bytes(&raw, req.max_read_bytes);
                TargetResult {
                    ok: true,
                    content: json!({
                        "path": req.path,
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
        let full =
            match resolve_path_scoped(&req.workdir, &req.path) {
                Ok(path) => path,
                Err(_) => return TargetResult::failed(
                    ExecTargetKind::Host,
                    "list_dir path must stay within workdir (no absolute paths or '..' traversal)"
                        .to_string(),
                    None,
                ),
            };
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
        let full = match resolve_path_scoped(&req.workdir, &req.path) {
            Ok(path) => path,
            Err(_) => return TargetResult::failed(
                ExecTargetKind::Host,
                "write_file path must stay within workdir (no absolute paths or '..' traversal)"
                    .to_string(),
                None,
            ),
        };
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
        let full = match resolve_path_scoped(&req.workdir, &req.path) {
            Ok(path) => path,
            Err(_) => return TargetResult::failed(
                ExecTargetKind::Host,
                "apply_patch path must stay within workdir (no absolute paths or '..' traversal)"
                    .to_string(),
                None,
            ),
        };
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
        let normalized_patch = normalize_patch_for_diffy(&req.patch, &req.path);
        let patched = match apply_patch_lenient(&original, &normalized_patch) {
            Ok(p) => p,
            Err(e) => return TargetResult::failed(ExecTargetKind::Host, e.to_string(), None),
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
            return Err(anyhow!(format!(
                "DOCKER_DAEMON_UNREACHABLE: docker execution target requested but Docker is unavailable: {}",
                stderr.trim()
            )));
        }
        Ok(())
    }

    pub fn validate_image_present_local(image: &str) -> anyhow::Result<()> {
        if image.trim().is_empty() {
            return Err(anyhow!(
                "DOCKER_SANDBOX_CONFIG_INVALID: docker image is required (pass --docker-image <image>)"
            ));
        }
        let out = std::process::Command::new("docker")
            .arg("image")
            .arg("inspect")
            .arg(image)
            .output()
            .context("failed to execute `docker image inspect`")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!(format!(
                "DOCKER_IMAGE_MISSING_LOCAL: docker image not available locally: {} (run `docker pull {}`)",
                stderr.trim(),
                image
            )));
        }
        Ok(())
    }

    fn validate_host_mount_path(host_workdir: &Path) -> anyhow::Result<()> {
        let path = host_workdir;
        if path.parent().is_none() {
            return Err(anyhow!(
                "DOCKER_SANDBOX_CONFIG_INVALID: refusing to mount filesystem root as docker workdir"
            ));
        }
        #[cfg(windows)]
        {
            use std::path::Component;
            let mut comps = path.components();
            if matches!(comps.next(), Some(Component::Prefix(_)))
                && matches!(comps.next(), Some(Component::RootDir))
                && comps.next().is_none()
            {
                return Err(anyhow!(
                    "DOCKER_SANDBOX_CONFIG_INVALID: refusing to mount drive root as docker workdir"
                ));
            }
        }
        Ok(())
    }

    fn docker_mount_arg(&self, host_workdir: &Path) -> anyhow::Result<String> {
        Self::validate_host_mount_path(host_workdir)?;
        Ok(format!(
            "{}:{}",
            host_workdir.to_string_lossy(),
            self.meta.workdir
        ))
    }

    fn build_run_command(
        &self,
        host_workdir: &Path,
        shell_script: &str,
    ) -> anyhow::Result<Command> {
        let mount = self.docker_mount_arg(host_workdir)?;
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
            .arg(shell_script);
        Ok(cmd)
    }

    #[cfg(test)]
    fn build_run_argv_for_test(
        &self,
        host_workdir: &Path,
        shell_script: &str,
    ) -> anyhow::Result<Vec<String>> {
        let mount = self.docker_mount_arg(host_workdir)?;
        let mut argv = vec![
            "docker".to_string(),
            "run".to_string(),
            "--rm".to_string(),
            "--network".to_string(),
            if self.meta.network == "none" {
                "none".to_string()
            } else {
                "bridge".to_string()
            },
        ];
        if let Some(user) = &self.meta.user {
            argv.push("--user".to_string());
            argv.push(user.clone());
        }
        argv.extend([
            "-v".to_string(),
            mount,
            "-w".to_string(),
            self.meta.workdir.clone(),
            self.meta.image.clone(),
            "sh".to_string(),
            "-lc".to_string(),
            shell_script.to_string(),
        ]);
        Ok(argv)
    }

    async fn run_container(
        &self,
        host_workdir: &Path,
        shell_script: &str,
        stdin_bytes: Option<&[u8]>,
        max_tool_output_bytes: usize,
    ) -> TargetResult {
        let mut cmd = match self.build_run_command(host_workdir, shell_script) {
            Ok(c) => c,
            Err(e) => {
                return TargetResult::failed(
                    ExecTargetKind::Docker,
                    e.to_string(),
                    Some(self.meta.clone()),
                )
            }
        };
        cmd.stdin(if stdin_bytes.is_some() {
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
                                format!(
                                    "DOCKER_SANDBOX_EXEC_FAILED: docker stdin write failed: {e}"
                                ),
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
                        format!("DOCKER_SANDBOX_EXEC_FAILED: docker command failed: {e}"),
                        Some(self.meta.clone()),
                    ),
                }
            }
            Err(e) => TargetResult::failed(
                ExecTargetKind::Docker,
                format!("DOCKER_SANDBOX_EXEC_FAILED: failed to spawn docker: {e}"),
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
        // Timeout enforcement is not implemented for the docker target (killing
        // the `docker run` client would not reliably stop the container). Rather
        // than silently ignore a requested bound, reject it with a structured,
        // classifiable error so the model/operator gets a clear signal instead
        // of an unexpectedly unbounded command.
        if req.timeout_ms > 0 {
            return TargetResult {
                ok: false,
                content: json!({
                    "error": "timeout_unsupported",
                    "execution_target": "docker",
                    "timeout_ms": req.timeout_ms,
                    "hint": "timeout_ms is not supported on the docker execution target. Re-run on the host target, or omit timeout_ms."
                })
                .to_string(),
                truncated: false,
                bytes: None,
                exit_code: None,
                stderr_truncated: None,
                stdout_truncated: None,
                execution_target: ExecTargetKind::Docker,
                docker: Some(self.meta.clone()),
            };
        }
        let args = req
            .args
            .iter()
            .map(|a| shell_escape(a))
            .collect::<Vec<_>>()
            .join(" ");
        let cwd = req.cwd.unwrap_or_else(|| ".".to_string());
        if !path_is_workdir_scoped(&cwd) {
            return TargetResult::failed(
                ExecTargetKind::Docker,
                "shell cwd must stay within workdir (no absolute paths or '..' traversal)"
                    .to_string(),
                Some(self.meta.clone()),
            );
        }
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
        if !path_is_workdir_scoped(&req.path) {
            return TargetResult::failed(
                ExecTargetKind::Docker,
                "read_file path must stay within workdir (no absolute paths or '..' traversal)"
                    .to_string(),
                Some(self.meta.clone()),
            );
        }
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
        if !path_is_workdir_scoped(&req.path) {
            return TargetResult::failed(
                ExecTargetKind::Docker,
                "list_dir path must stay within workdir (no absolute paths or '..' traversal)"
                    .to_string(),
                Some(self.meta.clone()),
            );
        }
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
        if !path_is_workdir_scoped(&req.path) {
            return TargetResult::failed(
                ExecTargetKind::Docker,
                "write_file path must stay within workdir (no absolute paths or '..' traversal)"
                    .to_string(),
                Some(self.meta.clone()),
            );
        }
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
        if !path_is_workdir_scoped(&req.path) {
            return TargetResult::failed(
                ExecTargetKind::Docker,
                "apply_patch path must stay within workdir (no absolute paths or '..' traversal)"
                    .to_string(),
                Some(self.meta.clone()),
            );
        }
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

/// Normalize a model-generated patch into valid unified diff format for `diffy`.
///
/// Local models commonly produce patches with:
/// - Missing `--- a/file` / `+++ b/file` headers
/// - Wrong line counts in `@@ -X,Y +X,Z @@` hunks
/// - No hunk header at all (just +/- lines)
///
/// Try diffy first, then fall back to search-and-replace using the -/+ lines.
fn apply_patch_lenient(original: &str, normalized_patch: &str) -> Result<String, String> {
    // Try strict diffy parse + apply first.
    if let Ok(patch) = diffy::Patch::from_str(normalized_patch) {
        if let Ok(result) = diffy::apply(original, &patch) {
            return Ok(result);
        }
    }

    // Fallback: extract old/new lines from the patch and do search-and-replace.
    let mut old_lines: Vec<String> = Vec::new();
    let mut new_lines: Vec<String> = Vec::new();
    for line in normalized_patch.lines() {
        if line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("@@ ")
            || line.starts_with("diff ")
        {
            continue;
        }
        if let Some(rest) = line.strip_prefix('-') {
            old_lines.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('+') {
            new_lines.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix(' ') {
            old_lines.push(rest.to_string());
            new_lines.push(rest.to_string());
        } else if !line.is_empty() {
            // Treat unpreixed lines as context.
            old_lines.push(line.to_string());
            new_lines.push(line.to_string());
        }
    }

    if old_lines.is_empty() {
        return Err("invalid patch: no content lines found".to_string());
    }

    let old_block = old_lines.join("\n");
    let new_block = new_lines.join("\n");

    if let Some(pos) = original.find(&old_block) {
        let mut result = String::with_capacity(original.len() + new_block.len());
        result.push_str(&original[..pos]);
        result.push_str(&new_block);
        result.push_str(&original[pos + old_block.len()..]);
        return Ok(result);
    }

    // Try trimmed matching (handle trailing whitespace differences).
    let old_trimmed: Vec<&str> = old_lines.iter().map(|l| l.trim_end()).collect();
    let orig_lines: Vec<&str> = original.lines().collect();
    for start in 0..orig_lines.len() {
        if start + old_trimmed.len() > orig_lines.len() {
            break;
        }
        let window = &orig_lines[start..start + old_trimmed.len()];
        if window
            .iter()
            .zip(&old_trimmed)
            .all(|(a, b)| a.trim_end() == *b)
        {
            let mut result = String::new();
            for line in &orig_lines[..start] {
                result.push_str(line);
                result.push('\n');
            }
            for line in &new_lines {
                result.push_str(line);
                result.push('\n');
            }
            for line in &orig_lines[start + old_trimmed.len()..] {
                result.push_str(line);
                result.push('\n');
            }
            return Ok(result);
        }
    }

    Err("failed to apply patch: could not locate the target content in the file".to_string())
}

/// This function fixes these issues so `diffy::Patch::from_str` can parse them.
fn normalize_patch_for_diffy(patch: &str, path: &str) -> String {
    let lines: Vec<&str> = patch.lines().collect();
    if lines.is_empty() {
        return patch.to_string();
    }

    // Check if this already looks like a valid unified diff that diffy can parse.
    if diffy::Patch::from_str(patch).is_ok() {
        return patch.to_string();
    }

    let mut has_file_headers = false;
    let mut hunk_lines: Vec<&str> = Vec::new();
    let mut pre_lines: Vec<String> = Vec::new();

    for line in &lines {
        if line.starts_with("--- ") {
            has_file_headers = true;
        }
    }

    // Collect lines that belong to the diff hunk (context, +, - lines).
    let mut in_hunk = false;
    for line in &lines {
        if line.starts_with("diff --git") || line.starts_with("--- ") || line.starts_with("+++ ") {
            pre_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("@@ ") {
            in_hunk = true;
            continue; // We'll regenerate the hunk header.
        }
        if in_hunk || line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
            in_hunk = true;
            hunk_lines.push(line);
        }
    }

    if hunk_lines.is_empty() {
        return patch.to_string();
    }

    // Count old/new lines for the hunk header.
    let mut old_count = 0u32;
    let mut new_count = 0u32;
    for line in &hunk_lines {
        if line.starts_with('-') {
            old_count += 1;
        } else if line.starts_with('+') {
            new_count += 1;
        } else {
            // Context line.
            old_count += 1;
            new_count += 1;
        }
    }

    let mut out = String::new();

    // Add file headers if missing.
    if !has_file_headers {
        out.push_str(&format!("--- a/{path}\n"));
        out.push_str(&format!("+++ b/{path}\n"));
    } else {
        for pl in &pre_lines {
            if pl.starts_with("--- ") || pl.starts_with("+++ ") {
                out.push_str(pl);
                out.push('\n');
            }
        }
    }

    // Write corrected hunk header.
    out.push_str(&format!("@@ -1,{old_count} +1,{new_count} @@\n"));

    // Write hunk body.
    for line in &hunk_lines {
        out.push_str(line);
        out.push('\n');
    }

    out
}

fn resolve_path_scoped(workdir: &Path, input: &str) -> anyhow::Result<PathBuf> {
    if !path_is_workdir_scoped(input) {
        return Err(anyhow!(
            "path must stay within workdir (no absolute paths or '..' traversal)"
        ));
    }
    Ok(resolve_path(workdir, input))
}

fn path_is_workdir_scoped(path: &str) -> bool {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        return false;
    }
    !p.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

/// Captured result of a managed child process, including whether the wait was
/// terminated by a timeout. On timeout, `status` is `None` and partial
/// stdout/stderr captured before termination are still returned.
struct ManagedOutput {
    status: Option<std::process::ExitStatus>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

/// Spawn `command` with piped stdout/stderr and `kill_on_drop(true)`, drain its
/// output concurrently, and wait for it to exit.
///
/// When `timeout_ms == 0` the wait is unbounded (historical behavior). When
/// `timeout_ms > 0` and the deadline elapses, the direct child is killed and
/// reaped, and `ManagedOutput::timed_out` is set.
///
/// Limitation: this kills and reaps the *direct* child only. A child launched
/// through a shell wrapper (e.g. `sh -c "..."` or `cmd /C "..."`) may leave
/// grandchild processes running until they exit on their own. Robust
/// process-tree termination (unix process groups / Windows job objects) is a
/// follow-up.
async fn spawn_and_wait_managed(
    mut command: Command,
    timeout_ms: u64,
    stdin_bytes: Option<&[u8]>,
    stream: Option<ShellOutputTx>,
) -> std::io::Result<ManagedOutput> {
    use std::sync::{Arc, Mutex};
    use tokio::io::AsyncReadExt;

    command
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn()?;

    if let Some(data) = stdin_bytes {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(data).await?;
            // `stdin` is dropped here, closing the pipe so the child sees EOF.
        }
    }

    // Drain stdout/stderr into shared buffers via incremental reads. Shared
    // buffers (rather than `read_to_end` returning the bytes) let the timeout
    // path recover whatever was captured so far even when the drain task is
    // still blocked on a pipe held open by a surviving grandchild process.
    //
    // When `stream` is provided, each read is also forwarded as a live
    // `ShellOutputChunk` (a best-effort, non-blocking send) so a consumer can
    // display progress while the command runs. The forwarded bytes never affect
    // the accumulated buffers or the final result envelope.
    fn drain<R>(
        pipe: Option<R>,
        stream: Option<ShellOutputTx>,
        kind: ShellStreamKind,
    ) -> (Arc<Mutex<Vec<u8>>>, tokio::task::JoinHandle<()>)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let handle = tokio::spawn(async move {
            if let Some(mut p) = pipe {
                let mut chunk = [0u8; 8192];
                loop {
                    match p.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut guard) = sink.lock() {
                                guard.extend_from_slice(&chunk[..n]);
                            }
                            if let Some(tx) = &stream {
                                // Ignore send errors: a dropped receiver just
                                // means nobody is watching the live stream.
                                let _ = tx.try_send(ShellOutputChunk {
                                    stream: kind,
                                    bytes: chunk[..n].to_vec(),
                                });
                            }
                        }
                    }
                }
            }
        });
        (buf, handle)
    }

    let (stdout_buf, stdout_task) =
        drain(child.stdout.take(), stream.clone(), ShellStreamKind::Stdout);
    let (stderr_buf, stderr_task) = drain(child.stderr.take(), stream, ShellStreamKind::Stderr);
    let stdout_abort = stdout_task.abort_handle();
    let stderr_abort = stderr_task.abort_handle();

    let (status, timed_out) = if timeout_ms == 0 {
        (Some(child.wait().await?), false)
    } else {
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), child.wait()).await
        {
            Ok(res) => (Some(res?), false),
            Err(_elapsed) => {
                // Best-effort terminate the direct child, then reap it.
                let _ = child.start_kill();
                let _ = child.wait().await;
                (None, true)
            }
        }
    };

    if timed_out {
        // The direct child is dead, but a grandchild may still hold the pipes
        // open. Give the drain tasks a short grace period to flush, then abort
        // so a surviving grandchild can never block our return.
        let grace = std::time::Duration::from_millis(200);
        let _ = tokio::time::timeout(grace, async {
            let _ = stdout_task.await;
            let _ = stderr_task.await;
        })
        .await;
        stdout_abort.abort();
        stderr_abort.abort();
    } else {
        // Normal exit: the pipes reach EOF, so the drain tasks complete.
        let _ = stdout_task.await;
        let _ = stderr_task.await;
    }

    let stdout = stdout_buf.lock().map(|g| g.clone()).unwrap_or_default();
    let stderr = stderr_buf.lock().map(|g| g.clone()).unwrap_or_default();

    Ok(ManagedOutput {
        status,
        stdout,
        stderr,
        timed_out,
    })
}

/// Build the standard shell `TargetResult` JSON envelope, adding timeout
/// metadata and an actionable recovery hint when the command was terminated by
/// a timeout.
fn build_shell_target_result(
    kind: ExecTargetKind,
    docker: Option<DockerMeta>,
    managed: ManagedOutput,
    timeout_ms: u64,
    max_tool_output_bytes: usize,
) -> TargetResult {
    let stdout_raw = String::from_utf8_lossy(&managed.stdout).to_string();
    let stderr_raw = String::from_utf8_lossy(&managed.stderr).to_string();
    // Middle truncation preserves both the leading context and the trailing
    // failure/summary that matter most for build/test output.
    let (stdout, stdout_truncated) =
        middle_truncate_utf8_to_bytes(&stdout_raw, max_tool_output_bytes);
    let (stderr, stderr_truncated) =
        middle_truncate_utf8_to_bytes(&stderr_raw, max_tool_output_bytes);
    let status_code = managed.status.and_then(|s| s.code());
    let ok = !managed.timed_out && managed.status.map(|s| s.success()).unwrap_or(false);
    let mut content = json!({
        "status": status_code,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
        "max_tool_output_bytes": max_tool_output_bytes
    });
    if managed.timed_out {
        if let Some(obj) = content.as_object_mut() {
            obj.insert("timed_out".to_string(), json!(true));
            obj.insert("timeout_ms".to_string(), json!(timeout_ms));
            obj.insert(
                "hint".to_string(),
                json!(format!(
                    "Shell command exceeded the {timeout_ms} ms timeout and was terminated before it finished. Re-run with a larger timeout_ms, make the command non-interactive, or narrow its scope."
                )),
            );
        }
    }
    TargetResult {
        ok,
        content: content.to_string(),
        truncated: stdout_truncated || stderr_truncated,
        bytes: Some((managed.stdout.len() + managed.stderr.len()) as u64),
        exit_code: status_code,
        stderr_truncated: Some(stderr_truncated),
        stdout_truncated: Some(stdout_truncated),
        execution_target: kind,
        docker,
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

/// Truncate `input` to at most `max_bytes` bytes while preserving both the head
/// and the tail of the content, joined by a marker describing the omitted
/// middle. This keeps the most useful part of build/test output (the leading
/// context and the trailing failure/summary) instead of discarding the tail.
///
/// Guarantees: the result is valid UTF-8 (never splits a codepoint), never
/// exceeds `max_bytes`, and is returned unchanged when `input` is within budget
/// or `max_bytes == 0` (parity with head truncation). If the budget is too
/// small to hold head + marker + tail, it falls back to head truncation.
fn middle_truncate_utf8_to_bytes(input: &str, max_bytes: usize) -> (String, bool) {
    if max_bytes == 0 || input.len() <= max_bytes {
        return (input.to_string(), false);
    }
    let marker = |omitted: usize| format!("\n[... truncated {omitted} bytes ...]\n");
    // Reserve using an upper bound on the omitted count: the true omitted count
    // is < input.len(), so its marker is never longer than this reservation.
    let reserve = marker(input.len()).len();
    if max_bytes <= reserve {
        // Not enough budget for a meaningful head+tail split; keep the head.
        return truncate_utf8_to_bytes(input, max_bytes);
    }
    let content_budget = max_bytes - reserve;
    let head_budget = content_budget / 2;
    let tail_budget = content_budget - head_budget;

    // Head: at most head_budget bytes, backing off to a char boundary.
    let mut head_end = head_budget.min(input.len());
    while head_end > 0 && !input.is_char_boundary(head_end) {
        head_end -= 1;
    }
    // Tail: at most tail_budget bytes from the end, advancing to a char boundary.
    let mut tail_start = input.len().saturating_sub(tail_budget);
    while tail_start < input.len() && !input.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    if tail_start < head_end {
        tail_start = head_end;
    }
    let omitted = tail_start - head_end;
    if omitted == 0 {
        // Unreachable when input exceeds budget, but stay budget-safe.
        return truncate_utf8_to_bytes(input, max_bytes);
    }
    let mut out = String::with_capacity(head_end + reserve + (input.len() - tail_start));
    out.push_str(&input[..head_end]);
    out.push_str(&marker(omitted));
    out.push_str(&input[tail_start..]);
    (out, true)
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        resolve_path_scoped, DockerTarget, ExecTargetKind, HostTarget, ReadReq, ShellReq,
        ShellStreamKind,
    };
    use crate::target::ExecTarget;
    use clap::ValueEnum;

    #[test]
    fn exec_target_kind_parse() {
        assert!(ExecTargetKind::from_str("host", true).is_ok());
        assert!(ExecTargetKind::from_str("docker", true).is_ok());
    }

    #[test]
    fn resolve_path_scoped_rejects_parent_and_absolute() {
        let workdir = PathBuf::from("workspace");
        assert!(resolve_path_scoped(&workdir, "../x").is_err());
        assert!(resolve_path_scoped(&workdir, "ok/file.txt").is_ok());
        let abs = if cfg!(windows) { "C:\\x" } else { "/x" };
        assert!(resolve_path_scoped(&workdir, abs).is_err());
    }

    #[tokio::test]
    async fn host_target_rejects_read_path_traversal() {
        let target = HostTarget;
        let out = target
            .read_file(ReadReq {
                workdir: PathBuf::from("."),
                path: "../secret.txt".to_string(),
                max_read_bytes: 200_000,
            })
            .await;
        assert!(!out.ok);
        assert!(out.content.contains("must stay within workdir"));
    }

    #[tokio::test]
    async fn host_target_rejects_shell_cwd_traversal() {
        let target = HostTarget;
        let out = target
            .exec_shell(ShellReq {
                workdir: PathBuf::from("."),
                cmd: "echo".to_string(),
                args: vec!["hi".to_string()],
                cwd: Some("../".to_string()),
                max_tool_output_bytes: 200_000,
                timeout_ms: 0,
                stream: None,
            })
            .await;
        assert!(!out.ok);
        assert!(out.content.contains("must stay within workdir"));
    }

    fn fast_echo_shell_req() -> ShellReq {
        if cfg!(windows) {
            ShellReq {
                workdir: PathBuf::from("."),
                cmd: "cmd".to_string(),
                args: vec!["/C".to_string(), "echo ok".to_string()],
                cwd: None,
                max_tool_output_bytes: 200_000,
                timeout_ms: 0,
                stream: None,
            }
        } else {
            ShellReq {
                workdir: PathBuf::from("."),
                cmd: "sh".to_string(),
                args: vec!["-c".to_string(), "echo ok".to_string()],
                cwd: None,
                max_tool_output_bytes: 200_000,
                timeout_ms: 0,
                stream: None,
            }
        }
    }

    fn slow_sleep_shell_req(timeout_ms: u64) -> ShellReq {
        if cfg!(windows) {
            ShellReq {
                workdir: PathBuf::from("."),
                cmd: "cmd".to_string(),
                args: vec!["/C".to_string(), "ping -n 6 127.0.0.1 >NUL".to_string()],
                cwd: None,
                max_tool_output_bytes: 200_000,
                timeout_ms,
                stream: None,
            }
        } else {
            ShellReq {
                workdir: PathBuf::from("."),
                cmd: "sh".to_string(),
                args: vec!["-c".to_string(), "sleep 5".to_string()],
                cwd: None,
                max_tool_output_bytes: 200_000,
                timeout_ms,
                stream: None,
            }
        }
    }

    fn stream_shell_req() -> ShellReq {
        if cfg!(windows) {
            ShellReq {
                workdir: PathBuf::from("."),
                cmd: "cmd".to_string(),
                args: vec![
                    "/C".to_string(),
                    "echo stream-out & echo stream-err 1>&2".to_string(),
                ],
                cwd: None,
                max_tool_output_bytes: 200_000,
                timeout_ms: 5_000,
                stream: None,
            }
        } else {
            ShellReq {
                workdir: PathBuf::from("."),
                cmd: "sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "printf stream-out; printf stream-err >&2".to_string(),
                ],
                cwd: None,
                max_tool_output_bytes: 200_000,
                timeout_ms: 5_000,
                stream: None,
            }
        }
    }

    #[tokio::test]
    async fn host_shell_under_timeout_succeeds_normally() {
        let mut req = fast_echo_shell_req();
        req.timeout_ms = 5_000;
        let out = HostTarget.exec_shell(req).await;
        assert!(out.ok, "fast command should succeed: {}", out.content);
        assert!(out.content.contains("ok"));
        assert!(!out.content.contains("timed_out"));
    }

    #[tokio::test]
    async fn host_shell_zero_timeout_is_unbounded_and_back_compatible() {
        // timeout_ms == 0 preserves historical unbounded behavior; a fast
        // command still completes normally.
        let out = HostTarget.exec_shell(fast_echo_shell_req()).await;
        assert!(out.ok, "command should succeed: {}", out.content);
        assert!(out.content.contains("ok"));
        assert!(!out.content.contains("timed_out"));
    }

    #[tokio::test]
    async fn host_shell_exceeding_timeout_returns_timeout_failure() {
        let start = std::time::Instant::now();
        let out = HostTarget.exec_shell(slow_sleep_shell_req(200)).await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "timed-out command should return promptly, took {elapsed:?}"
        );
        assert!(!out.ok, "timed-out command must not report success");
        let parsed: serde_json::Value =
            serde_json::from_str(&out.content).expect("timeout content is JSON");
        assert_eq!(parsed["timed_out"], serde_json::json!(true));
        assert_eq!(parsed["timeout_ms"], serde_json::json!(200));
        let hint = parsed["hint"].as_str().unwrap_or_default();
        assert!(
            hint.contains("timeout_ms"),
            "timeout result should include an actionable hint, got: {hint}"
        );
    }

    #[tokio::test]
    async fn host_shell_streaming_preserves_final_result_envelope_fields() {
        let plain = HostTarget.exec_shell(stream_shell_req()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let mut streamed_req = stream_shell_req();
        streamed_req.stream = Some(tx);
        let streamed = HostTarget.exec_shell(streamed_req).await;

        assert!(plain.ok, "plain command should succeed: {}", plain.content);
        assert!(
            streamed.ok,
            "streamed command should succeed: {}",
            streamed.content
        );
        let plain_json: serde_json::Value =
            serde_json::from_str(&plain.content).expect("plain shell result JSON");
        let streamed_json: serde_json::Value =
            serde_json::from_str(&streamed.content).expect("streamed shell result JSON");
        for key in [
            "status",
            "stdout",
            "stderr",
            "stdout_truncated",
            "stderr_truncated",
            "max_tool_output_bytes",
        ] {
            assert_eq!(streamed_json[key], plain_json[key], "field {key}");
        }
        assert_eq!(streamed.stdout_truncated, plain.stdout_truncated);
        assert_eq!(streamed.stderr_truncated, plain.stderr_truncated);
        assert_eq!(streamed.exit_code, plain.exit_code);

        let mut live_stdout = Vec::new();
        let mut live_stderr = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            match chunk.stream {
                ShellStreamKind::Stdout => live_stdout.extend_from_slice(&chunk.bytes),
                ShellStreamKind::Stderr => live_stderr.extend_from_slice(&chunk.bytes),
            }
        }
        assert!(
            String::from_utf8_lossy(&live_stdout).contains("stream-out"),
            "stdout chunks: {:?}",
            String::from_utf8_lossy(&live_stdout)
        );
        assert!(
            String::from_utf8_lossy(&live_stderr).contains("stream-err"),
            "stderr chunks: {:?}",
            String::from_utf8_lossy(&live_stderr)
        );
    }

    #[test]
    fn docker_command_assembly_is_deterministic() {
        let t = DockerTarget::new(
            "ubuntu:24.04".to_string(),
            "/work".to_string(),
            "none".to_string(),
            Some("1000:1000".to_string()),
        );
        let argv = t
            .build_run_argv_for_test(&PathBuf::from("C:/demo"), "echo hi")
            .expect("argv");
        assert_eq!(
            argv,
            vec![
                "docker",
                "run",
                "--rm",
                "--network",
                "none",
                "--user",
                "1000:1000",
                "-v",
                "C:/demo:/work",
                "-w",
                "/work",
                "ubuntu:24.04",
                "sh",
                "-lc",
                "echo hi"
            ]
        );
    }

    #[tokio::test]
    async fn docker_shell_rejects_timeout_with_structured_error() {
        // The guard returns before any docker invocation, so this is
        // deterministic without docker installed.
        let t = DockerTarget::new(
            "ubuntu:24.04".to_string(),
            "/work".to_string(),
            "none".to_string(),
            None,
        );
        let out = t
            .exec_shell(ShellReq {
                workdir: PathBuf::from("."),
                cmd: "echo".to_string(),
                args: vec!["hi".to_string()],
                cwd: None,
                max_tool_output_bytes: 200_000,
                timeout_ms: 500,
                stream: None,
            })
            .await;
        assert!(!out.ok, "timeout on docker target must be rejected");
        let parsed: serde_json::Value =
            serde_json::from_str(&out.content).expect("structured docker rejection is JSON");
        assert_eq!(parsed["error"], serde_json::json!("timeout_unsupported"));
        assert_eq!(parsed["execution_target"], serde_json::json!("docker"));
        assert_eq!(parsed["timeout_ms"], serde_json::json!(500));
    }

    #[test]
    fn docker_mount_rejects_root_paths() {
        let t = DockerTarget::new(
            "ubuntu:24.04".to_string(),
            "/work".to_string(),
            "none".to_string(),
            None,
        );
        let root = if cfg!(windows) {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        let err = t.docker_mount_arg(&root).expect_err("should reject root");
        assert!(err.to_string().contains("DOCKER_SANDBOX_CONFIG_INVALID"));
    }

    #[test]
    fn middle_truncate_leaves_short_output_unchanged() {
        let input = "short output";
        let (out, truncated) = super::middle_truncate_utf8_to_bytes(input, 1000);
        assert_eq!(out, input);
        assert!(!truncated);
        // max_bytes == 0 disables truncation (parity with head truncation).
        let (out0, t0) = super::middle_truncate_utf8_to_bytes(input, 0);
        assert_eq!(out0, input);
        assert!(!t0);
    }

    #[test]
    fn middle_truncate_preserves_head_and_tail_within_budget() {
        let head = "HEAD_START_".repeat(50); // 550 bytes
        let tail = "_TAIL_END".repeat(50); // 450 bytes
        let input = format!("{head}{}{tail}", "x".repeat(5000));
        let max_bytes = 400;
        let (out, truncated) = super::middle_truncate_utf8_to_bytes(&input, max_bytes);
        assert!(truncated);
        assert!(
            out.len() <= max_bytes,
            "output {} exceeded budget {max_bytes}",
            out.len()
        );
        assert!(out.starts_with("HEAD_START_"), "head not preserved: {out}");
        assert!(out.ends_with("_TAIL_END"), "tail not preserved: {out}");
        assert!(
            out.contains("[... truncated "),
            "missing truncation marker: {out}"
        );
    }

    #[test]
    fn middle_truncate_marker_reports_omitted_byte_count() {
        let input = "a".repeat(2000);
        let max_bytes = 200;
        let (out, truncated) = super::middle_truncate_utf8_to_bytes(&input, max_bytes);
        assert!(truncated);
        // Extract the reported omitted byte count from the marker.
        let start = out.find("[... truncated ").expect("marker present") + "[... truncated ".len();
        let rest = &out[start..];
        let end = rest.find(" bytes ...]").expect("marker suffix present");
        let reported: usize = rest[..end].parse().expect("omitted count is a number");
        // Reported omitted count plus the kept head+tail must reconstruct the
        // original length exactly.
        let marker_len = format!("\n[... truncated {reported} bytes ...]\n").len();
        let kept = out.len() - marker_len;
        assert_eq!(reported + kept, input.len());
        assert!(reported > 0);
    }

    #[test]
    fn middle_truncate_keeps_utf8_valid_and_within_budget() {
        // 3-byte codepoints stress char-boundary handling.
        let input = "€".repeat(4000); // 12000 bytes
        let max_bytes = 500;
        let (out, truncated) = super::middle_truncate_utf8_to_bytes(&input, max_bytes);
        assert!(truncated);
        assert!(out.len() <= max_bytes, "over budget: {}", out.len());
        // String is valid UTF-8 by construction; verify no replacement/garbage
        // and that every non-marker char is the intended codepoint.
        for ch in out.chars() {
            assert!(
                ch == '€' || "\n[...truncatedbytes ]0123456789".contains(ch),
                "unexpected char in output: {ch:?}"
            );
        }
        assert!(out.starts_with('€'));
        assert!(out.ends_with('€'));
    }

    #[test]
    fn build_shell_result_middle_truncates_stdout() {
        let head = "START".repeat(40);
        let tail = "ENDLINE_FAILURE".repeat(40);
        let raw = format!("{head}{}{tail}", "m".repeat(3000));
        let managed = super::ManagedOutput {
            status: None,
            stdout: raw.clone().into_bytes(),
            stderr: Vec::new(),
            timed_out: false,
        };
        let max_bytes = 300;
        let out = super::build_shell_target_result(
            super::ExecTargetKind::Host,
            None,
            managed,
            0,
            max_bytes,
        );
        let parsed: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        let stdout = parsed["stdout"].as_str().expect("stdout string");
        assert_eq!(parsed["stdout_truncated"], serde_json::json!(true));
        assert!(stdout.len() <= max_bytes);
        assert!(stdout.starts_with("START"));
        assert!(stdout.ends_with("ENDLINE_FAILURE"));
        assert!(stdout.contains("[... truncated "));
    }

    #[test]
    fn build_shell_result_preserves_small_timeout_partial_output() {
        // Partial output under budget must pass through unchanged even on the
        // timeout path.
        let managed = super::ManagedOutput {
            status: None,
            stdout: b"partial line before kill".to_vec(),
            stderr: b"warn: interrupted".to_vec(),
            timed_out: true,
        };
        let out = super::build_shell_target_result(
            super::ExecTargetKind::Host,
            None,
            managed,
            200,
            200_000,
        );
        let parsed: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        assert_eq!(parsed["timed_out"], serde_json::json!(true));
        assert_eq!(
            parsed["stdout"],
            serde_json::json!("partial line before kill")
        );
        assert_eq!(parsed["stderr"], serde_json::json!("warn: interrupted"));
        assert_eq!(parsed["stdout_truncated"], serde_json::json!(false));
    }
}
