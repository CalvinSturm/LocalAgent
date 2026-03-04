param(
    [string]$Workdir = ".tmp/manual-tui-testing/workdir",
    [string]$StateDir = ".tmp/manual-tui-testing/state",
    [switch]$Force,
    [switch]$NoSeedFixture
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path $Workdir | Out-Null
New-Item -ItemType Directory -Force -Path $StateDir | Out-Null

$args = @(
    "run", "--",
    "init",
    "--workdir", $Workdir,
    "--state-dir", $StateDir
)
if ($Force) { $args += "--force" }

Write-Host ("cargo " + ($args -join " "))
cargo @args
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (-not $NoSeedFixture) {
    $providersDir = Join-Path $Workdir "src/providers"
    $coreDir = Join-Path $Workdir "src/core"
    $fixturesDir = Join-Path $Workdir "fixtures"
    New-Item -ItemType Directory -Force -Path $providersDir | Out-Null
    New-Item -ItemType Directory -Force -Path $coreDir | Out-Null
    New-Item -ItemType Directory -Force -Path $fixturesDir | Out-Null

    @'
[package]
name = "manual-pr8-pr12-fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
'@ | Set-Content (Join-Path $Workdir "Cargo.toml")

    @'
pub mod providers;
pub mod core;
'@ | Set-Content (Join-Path $Workdir "src/lib.rs")

    @'
pub mod common;
pub mod http;
pub mod mock;
pub mod ollama;
pub mod openai_compat;
'@ | Set-Content (Join-Path $providersDir "mod.rs")

    @'
pub fn normalize_response(input: &str) -> String {
    input.trim().to_string()
}
'@ | Set-Content (Join-Path $providersDir "common.rs")

    @'
pub struct HttpConfig {
    pub timeout_ms: u64,
}
'@ | Set-Content (Join-Path $providersDir "http.rs")

    @'
pub struct MockProvider;
'@ | Set-Content (Join-Path $providersDir "mock.rs")

    @'
pub struct OllamaProvider;
'@ | Set-Content (Join-Path $providersDir "ollama.rs")

    @'
pub struct OpenAiCompatProvider;
'@ | Set-Content (Join-Path $providersDir "openai_compat.rs")

    @'
pub fn planner_entry() {
    // TODO: stabilize planner handoff payload.
}
'@ | Set-Content (Join-Path $coreDir "planner.rs")

    @'
pub fn gate_entry() {
    // TODO: review runtime gate reason mapping.
}
'@ | Set-Content (Join-Path $coreDir "gate.rs")

    @'
pub mod planner;
pub mod gate;
'@ | Set-Content (Join-Path $coreDir "mod.rs")

    @'
# Manual PR8-PR12 Fixture

TODO: validate unknown tool fallback path using read_file/list_dir.
TODO: validate invalid args repair flow against Cargo.toml.
TODO: validate recursive glob+grep coverage over src/**/*.rs.
'@ | Set-Content (Join-Path $Workdir "README.md")

    [byte[]]$bytes = 0,159,146,150,0,255,1,2,3
    [System.IO.File]::WriteAllBytes((Join-Path $fixturesDir "binary.bin"), $bytes)
}

Write-Host "Setup complete."
Write-Host ("Workdir:   " + (Resolve-Path $Workdir))
Write-Host ("State dir: " + (Resolve-Path $StateDir))
if (-not $NoSeedFixture) {
    Write-Host "Fixture seeded: src/providers/*, src/core/*, Cargo.toml, README.md, fixtures/binary.bin"
}
