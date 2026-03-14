param(
    [string]$Model = "omnicoder-9b@q8_0",

    [ValidateSet("lmstudio", "ollama", "llamacpp", "mock")]
    [string]$Provider = "lmstudio",

    [string]$BaseUrl = "http://localhost:1234/v1",

    [string]$InstructionsConfig = "scripts/configs/omnicoder_validation_discipline_instructions.yaml",

    [string]$InstructionTaskProfile = "validation_shell_only_v1",

    [string]$RootDir = ".artifacts/manual/omnicoder_validation_discipline_comparison",

    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-CommandPrefix {
    if (Test-Path "Cargo.toml") {
        return @("cargo", "run", "--")
    }

    $command = Get-Command "localagent" -ErrorAction SilentlyContinue
    if ($command) {
        return ,("localagent")
    }

    throw "Neither a local Cargo.toml checkout nor a localagent binary on PATH was found."
}

function Invoke-LocalAgent {
    param([string[]]$CommandArgs)

    $prefix = @(Resolve-CommandPrefix)
    $preview = ($prefix + $CommandArgs) -join " "
    Write-Host $preview

    if ($DryRun) {
        return 0
    }

    if ($prefix.Count -eq 1) {
        & $prefix[0] @CommandArgs
    }
    else {
        & $prefix[0] $prefix[1] $prefix[2] @CommandArgs
    }

    return $LASTEXITCODE
}

function Write-RepoFiles {
    param(
        [string]$Workdir,
        [object[]]$Files
    )

    foreach ($file in $Files) {
        $fullPath = Join-Path $Workdir $file.Path
        $parent = Split-Path -Parent $fullPath
        if ($parent) {
            New-Item -ItemType Directory -Force -Path $parent | Out-Null
        }
        Set-Content -Path $fullPath -Value $file.Content
    }
}

function Write-U5Fixture {
    param([string]$Workdir)

    Write-RepoFiles -Workdir $Workdir -Files @(
        @{ Path = "Cargo.toml"; Content = @'
[package]
name = "cli_bugfix"
version = "0.1.0"
edition = "2021"
'@ },
        @{ Path = "src/lib.rs"; Content = @'
pub fn parse_count(input: &str) -> Result<u32, String> {
    // Bug: this rejects inputs with surrounding spaces.
    if input.chars().all(|c| c.is_ascii_digit()) {
        input.parse::<u32>().map_err(|e| e.to_string())
    } else {
        Err("invalid number".to_string())
    }
}
'@ },
        @{ Path = "src/main.rs"; Content = @'
fn main() {
    let _ = cli_bugfix::parse_count("7");
}
'@ },
        @{ Path = "tests/regression.rs"; Content = @'
use cli_bugfix::parse_count;

#[test]
fn parses_simple_count() {
    assert_eq!(parse_count("12").unwrap(), 12);
}

#[test]
fn parses_spaced_count() {
    assert_eq!(parse_count(" 12 ").unwrap(), 12);
}
'@ },
        @{ Path = "README.md"; Content = "# CLI bugfix fixture`n" }
    )
}

function Write-U6Fixture {
    param([string]$Workdir)

    Write-RepoFiles -Workdir $Workdir -Files @(
        @{ Path = "Cargo.toml"; Content = @'
[package]
name = "recovery_bugfix"
version = "0.1.0"
edition = "2021"
'@ },
        @{ Path = "src/main.rs"; Content = @'
fn main() {
    let _ = recovery_bugfix::parse_count("7");
}
'@ },
        @{ Path = "src/lib.rs"; Content = @'
pub mod parser;

pub use parser::parse_count;
'@ },
        @{ Path = "src/parser.rs"; Content = @'
pub fn parse_count(input: &str) -> Result<u32, String> {
    if input.chars().all(|c| c.is_ascii_digit()) {
        input.parse::<u32>().map_err(|e| e.to_string())
    } else {
        Err("invalid number".to_string())
    }
}
'@ },
        @{ Path = "tests/regression.rs"; Content = @'
use recovery_bugfix::parse_count;

#[test]
fn parses_simple_count() {
    assert_eq!(parse_count("12").unwrap(), 12);
}

#[test]
fn parses_spaced_count() {
    assert_eq!(parse_count(" 12 ").unwrap(), 12);
}
'@ },
        @{ Path = "README.md"; Content = @'
# Recovery bugfix fixture

The parser lives in src/parser.rs.
'@ }
    )
}

function Write-U7Fixture {
    param([string]$Workdir)

    Write-RepoFiles -Workdir $Workdir -Files @(
        @{ Path = "Cargo.toml"; Content = @'
[package]
name = "multi_file_feature"
version = "0.1.0"
edition = "2021"
'@ },
        @{ Path = "src/lib.rs"; Content = @'
pub fn is_even(value: i32) -> bool {
    value % 2 == 0
}
'@ },
        @{ Path = "tests/regression.rs"; Content = @'
use multi_file_feature::is_even;

#[test]
fn even_values_are_true() {
    assert!(is_even(4));
}
'@ },
        @{ Path = "README.md"; Content = "# Multi-file feature fixture`n" }
    )
}

function Write-U9Fixture {
    param([string]$Workdir)

    Write-RepoFiles -Workdir $Workdir -Files @(
        @{ Path = "Cargo.toml"; Content = @'
[package]
name = "test_repair_fixture"
version = "0.1.0"
edition = "2021"
'@ },
        @{ Path = "src/lib.rs"; Content = @'
pub fn total(a: i32, b: i32) -> i32 {
    a + b
}
'@ },
        @{ Path = "tests/regression.rs"; Content = @'
use test_repair_fixture::total;

#[test]
fn total_adds_values() {
    assert_eq!(total(2, 3), 6);
}
'@ },
        @{ Path = "README.md"; Content = "# Test repair fixture`n" }
    )
}

function Write-U12Fixture {
    param([string]$Workdir)

    Write-RepoFiles -Workdir $Workdir -Files @(
        @{ Path = "Cargo.toml"; Content = @'
[package]
name = "closeout_quality_bugfix"
version = "0.1.0"
edition = "2021"
'@ },
        @{ Path = "src/lib.rs"; Content = @'
pub mod math;
'@ },
        @{ Path = "src/math.rs"; Content = @'
pub fn total(a: i32, b: i32) -> i32 {
    a - b
}
'@ },
        @{ Path = "tests/regression.rs"; Content = @'
use closeout_quality_bugfix::math::total;

#[test]
fn total_adds_values() {
    assert_eq!(total(2, 3), 5);
}
'@ },
        @{ Path = "README.md"; Content = "# Closeout quality bugfix fixture`n" }
    )
}

function Initialize-Case {
    param(
        [string]$CaseRoot,
        [scriptblock]$FixtureWriter
    )

    if (-not $DryRun) {
        New-Item -ItemType Directory -Force -Path $CaseRoot | Out-Null
        $workdir = Join-Path $CaseRoot "workdir"
        New-Item -ItemType Directory -Force -Path $workdir | Out-Null
        & $FixtureWriter $workdir
        return $workdir
    }

    return (Join-Path $CaseRoot "workdir")
}

function Run-Scenario {
    param(
        [string]$TaskId,
        [string]$Variant,
        [string]$Prompt,
        [scriptblock]$FixtureWriter,
        [switch]$UseProfile
    )

    $caseRoot = Join-Path $RootDir "$TaskId-$Variant"
    $workdir = Initialize-Case -CaseRoot $caseRoot -FixtureWriter $FixtureWriter
    $stateDir = Join-Path $caseRoot "state"
    $logPath = Join-Path $caseRoot "stdout.log"

    $cmdArgs = [System.Collections.Generic.List[string]]::new()
    $cmdArgs.Add("--provider")
    $cmdArgs.Add($Provider)
    $cmdArgs.Add("--base-url")
    $cmdArgs.Add($BaseUrl)
    $cmdArgs.Add("--model")
    $cmdArgs.Add($Model)
    $cmdArgs.Add("--allow-write")
    $cmdArgs.Add("--allow-shell")
    $cmdArgs.Add("--enable-write-tools")
    $cmdArgs.Add("--state-dir")
    $cmdArgs.Add($stateDir)
    $cmdArgs.Add("--workdir")
    $cmdArgs.Add($workdir)
    $cmdArgs.Add("--no-session")
    if ($UseProfile) {
        $cmdArgs.Add("--instructions-config")
        $cmdArgs.Add($InstructionsConfig)
        $cmdArgs.Add("--instruction-task-profile")
        $cmdArgs.Add($InstructionTaskProfile)
    }
    $cmdArgs.Add("--prompt")
    $cmdArgs.Add($Prompt)
    $cmdArgs.Add("run")

    if ($DryRun) {
        [void](Invoke-LocalAgent -CommandArgs $cmdArgs.ToArray())
        return
    }

    $lines = & {
        $exit = Invoke-LocalAgent -CommandArgs $cmdArgs.ToArray() 2>&1
        $global:LAST_SCRIPT_EXIT = $LASTEXITCODE
        $exit
    }
    $exitCode = $LAST_SCRIPT_EXIT
    @("exit_code=$exitCode") + ($lines | ForEach-Object { "$_" }) | Set-Content -Path $logPath
    if ($exitCode -ne 0) {
        Write-Warning "$TaskId/$Variant exited with code $exitCode. See $logPath."
    }
}

$u5Prompt = 'Inspect this crate, fix the parser bug causing spaced numbers to fail, run `cargo test`, and reply with exactly `validated: src/lib.rs` only if the tests pass.'
$u6Prompt = 'Update the parser so it trims whitespace before parsing. If a path guess fails, inspect the repo and recover. Run `cargo test` and reply with exactly `validated: src/parser.rs` only if tests pass.'
$u7Prompt = 'Add a new helper `is_zero_or_even` in `src/lib.rs`, update the tests in `tests/regression.rs` to validate both `0` and `4`, run `cargo test`, and reply with exactly `validated: src/lib.rs, tests/regression.rs` only if the tests pass.'
$u9Prompt = 'Inspect this crate, repair the broken existing unit test so it matches the current implementation, run `cargo test`, and reply with exactly `validated: tests/regression.rs` only if the tests pass.'
$u12Prompt = 'Inspect the code, fix the bug so `total(2, 3)` would produce `5`, run `cargo test`, and then reply with a concise final answer that mentions `src/math.rs` and that `cargo test passed`.'

if (-not $DryRun) {
    New-Item -ItemType Directory -Force -Path $RootDir | Out-Null
}

Run-Scenario -TaskId "U5" -Variant "baseline" -Prompt $u5Prompt -FixtureWriter ${function:Write-U5Fixture}
Run-Scenario -TaskId "U5" -Variant "shaped" -Prompt $u5Prompt -FixtureWriter ${function:Write-U5Fixture} -UseProfile
Run-Scenario -TaskId "U6" -Variant "baseline" -Prompt $u6Prompt -FixtureWriter ${function:Write-U6Fixture}
Run-Scenario -TaskId "U6" -Variant "shaped" -Prompt $u6Prompt -FixtureWriter ${function:Write-U6Fixture} -UseProfile
Run-Scenario -TaskId "U7" -Variant "baseline" -Prompt $u7Prompt -FixtureWriter ${function:Write-U7Fixture}
Run-Scenario -TaskId "U7" -Variant "shaped" -Prompt $u7Prompt -FixtureWriter ${function:Write-U7Fixture} -UseProfile
Run-Scenario -TaskId "U9" -Variant "baseline" -Prompt $u9Prompt -FixtureWriter ${function:Write-U9Fixture}
Run-Scenario -TaskId "U9" -Variant "shaped" -Prompt $u9Prompt -FixtureWriter ${function:Write-U9Fixture} -UseProfile
Run-Scenario -TaskId "U12" -Variant "baseline" -Prompt $u12Prompt -FixtureWriter ${function:Write-U12Fixture}
Run-Scenario -TaskId "U12" -Variant "shaped" -Prompt $u12Prompt -FixtureWriter ${function:Write-U12Fixture} -UseProfile
