param(
    [Parameter(Mandatory = $true)]
    [string]$Model,

    [ValidateSet("lmstudio", "ollama", "llamacpp", "mock")]
    [string]$Provider = "lmstudio",

    [string]$BaseUrl = "http://localhost:1234/v1",

    [string]$InstructionsConfig = "scripts/configs/u3_u4_closeout_instructions.yaml",

    [string]$InstructionTaskProfile = "closeout_exact_answer_v1",

    [string]$RootDir = ".artifacts/manual/u3_u4_closeout_comparison",

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

function Write-U3Fixture {
    param([string]$Workdir)

    New-Item -ItemType Directory -Force -Path (Join-Path $Workdir "src") | Out-Null
    Set-Content -Path (Join-Path $Workdir "Cargo.toml") -Value @'
[package]
name = "single_file_bugfix"
version = "0.1.0"
edition = "2021"
'@
    Set-Content -Path (Join-Path $Workdir "src/math.rs") -Value @'
pub fn total(a: i32, b: i32) -> i32 {
    a - b
}
'@
    Set-Content -Path (Join-Path $Workdir "src/main.rs") -Value @'
mod math;

fn main() {
    println!("{}", math::total(2, 3));
}
'@
    Set-Content -Path (Join-Path $Workdir "README.md") -Value "# Single-file bugfix fixture`n"
}

function Write-U4Fixture {
    param([string]$Workdir)

    New-Item -ItemType Directory -Force -Path (Join-Path $Workdir "src") | Out-Null
    Set-Content -Path (Join-Path $Workdir "Cargo.toml") -Value @'
[package]
name = "inspect_before_edit"
version = "0.1.0"
edition = "2021"
'@
    Set-Content -Path (Join-Path $Workdir "src/main.rs") -Value @'
mod messages;

fn main() {
    println!("{}", messages::greeting());
}
'@
    Set-Content -Path (Join-Path $Workdir "src/messages.rs") -Value @'
pub fn greeting() -> &'static str {
    "helo"
}
'@
    Set-Content -Path (Join-Path $Workdir "src/unused.rs") -Value @'
pub const NOISE: &str = "ignore me";
'@
    Set-Content -Path (Join-Path $Workdir "README.md") -Value @'
# Inspect-before-edit fixture

Find the real greeting definition before editing.
'@
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

$u3Prompt = 'Inspect the code, fix the bug so `total(2, 3)` would produce `5`, and reply with exactly `fixed: src/math.rs`.'
$u4Prompt = 'Find where the visible greeting typo is actually defined, fix `helo` to `hello`, and reply with exactly `fixed: src/messages.rs`.'

if (-not $DryRun) {
    New-Item -ItemType Directory -Force -Path $RootDir | Out-Null
}

Run-Scenario -TaskId "U3" -Variant "baseline" -Prompt $u3Prompt -FixtureWriter ${function:Write-U3Fixture}
Run-Scenario -TaskId "U3" -Variant "shaped" -Prompt $u3Prompt -FixtureWriter ${function:Write-U3Fixture} -UseProfile
Run-Scenario -TaskId "U4" -Variant "baseline" -Prompt $u4Prompt -FixtureWriter ${function:Write-U4Fixture}
Run-Scenario -TaskId "U4" -Variant "shaped" -Prompt $u4Prompt -FixtureWriter ${function:Write-U4Fixture} -UseProfile
