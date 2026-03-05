param(
    [string]$OutDir = ".tmp/manual-testing/eval-coding-pack"
)

$ErrorActionPreference = "Stop"

function Write-Utf8File {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content
    )
    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Reset-Dir {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (Test-Path $Path) {
        Remove-Item -Recurse -Force $Path
    }
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
}

function New-TaskDir {
    param(
        [Parameter(Mandatory = $true)][string]$Base,
        [Parameter(Mandatory = $true)][string]$TaskId,
        [Parameter(Mandatory = $true)][string]$Prompt
    )
    $taskDir = Join-Path $Base $TaskId
    New-Item -ItemType Directory -Force -Path $taskDir | Out-Null
    Write-Utf8File -Path (Join-Path $taskDir "PROMPT.txt") -Content ($Prompt + "`n")
    return $taskDir
}

function Seed-CliBugfixFixture {
    param([Parameter(Mandatory = $true)][string]$TaskDir)
    Write-Utf8File -Path (Join-Path $TaskDir "Cargo.toml") -Content @"
[package]
name = "cli_bugfix"
version = "0.1.0"
edition = "2021"
"@
    Write-Utf8File -Path (Join-Path $TaskDir "src/lib.rs") -Content @"
pub fn parse_count(input: &str) -> Result<u32, String> {
    // Bug: this rejects inputs with surrounding spaces.
    if input.chars().all(|c| c.is_ascii_digit()) {
        input.parse::<u32>().map_err(|e| e.to_string())
    } else {
        Err("invalid number".to_string())
    }
}
"@
    Write-Utf8File -Path (Join-Path $TaskDir "src/main.rs") -Content @"
fn main() {
    let _ = cli_bugfix::parse_count("7");
}
"@
    Write-Utf8File -Path (Join-Path $TaskDir "tests/regression.rs") -Content @"
use cli_bugfix::parse_count;

#[test]
fn parses_simple_count() {
    assert_eq!(parse_count("12").unwrap(), 12);
}

#[test]
fn parses_spaced_count() {
    assert_eq!(parse_count(" 12 ").unwrap(), 12);
}
"@
    Write-Utf8File -Path (Join-Path $TaskDir "README.md") -Content @"
# CLI bugfix fixture
"@
}

function Seed-WorkspaceRefactorFixture {
    param([Parameter(Mandatory = $true)][string]$TaskDir)
    Write-Utf8File -Path (Join-Path $TaskDir "Cargo.toml") -Content @"
[workspace]
members = ["crates/libcore", "crates/app"]
resolver = "2"
"@
    Write-Utf8File -Path (Join-Path $TaskDir "README.md") -Content @"
# Workspace Fixture

TODO: add refactor note.
"@
    Write-Utf8File -Path (Join-Path $TaskDir "crates/libcore/Cargo.toml") -Content @"
[package]
name = "libcore"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
"@
    Write-Utf8File -Path (Join-Path $TaskDir "crates/libcore/src/lib.rs") -Content @"
pub fn combine(a: i32, b: i32) -> i32 {
    // TODO: fix implementation and refactor signature
    a - b
}
"@
    Write-Utf8File -Path (Join-Path $TaskDir "crates/libcore/tests/basic.rs") -Content @"
use libcore::combine;

#[test]
fn combine_adds_values() {
    assert_eq!(combine(2, 3), 5);
}
"@
    Write-Utf8File -Path (Join-Path $TaskDir "crates/app/Cargo.toml") -Content @"
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
libcore = { path = "../libcore" }
"@
    Write-Utf8File -Path (Join-Path $TaskDir "crates/app/src/main.rs") -Content @"
fn main() {
    let v = libcore::combine(10, 5);
    println!("{v}");
}
"@
}

$outAbs = Resolve-Path -LiteralPath "." | ForEach-Object { Join-Path $_.Path $OutDir }
Reset-Dir -Path $outAbs

$prompts = @(
    @{
        Id = "C1"
        Prompt = "Create a new file at src/hello.txt containing exactly hello followed by a newline. Use the write_file tool. Then respond with a brief confirmation."
    },
    @{
        Id = "C2"
        Prompt = "Edit main.rs by using apply_patch so that fn answer() returns 2 instead of 1. Do not rewrite the whole file with write_file. Then confirm done."
    },
    @{
        Id = "C3"
        Prompt = "In this crate, fix the parsing bug so all tests pass, then run cargo test and summarize the result."
    },
    @{
        Id = "C4"
        Prompt = "You are in a Rust workspace fixture. Fix the failing test, refactor libcore::combine signature from two i32 args to one tuple argument across both crates, and update README with a short line starting with 'Refactor note:'. Prefer apply_patch for edits. After edits, run cargo test and report success."
    },
    @{
        Id = "C5"
        Prompt = "Fix the parsing bug in this CLI fixture and add one additional regression test named parses_spaced_count_extra in tests/regression.rs. Keep the behavior deterministic and then summarize what changed."
    }
)

foreach ($item in $prompts) {
    $taskDir = New-TaskDir -Base $outAbs -TaskId $item.Id -Prompt $item.Prompt
    switch ($item.Id) {
        "C1" {
            New-Item -ItemType Directory -Force -Path (Join-Path $taskDir "src") | Out-Null
        }
        "C2" {
            Write-Utf8File -Path (Join-Path $taskDir "main.rs") -Content @"
fn answer() -> i32 {
    return 1;
}
"@
        }
        "C3" { Seed-CliBugfixFixture -TaskDir $taskDir }
        "C4" { Seed-WorkspaceRefactorFixture -TaskDir $taskDir }
        "C5" { Seed-CliBugfixFixture -TaskDir $taskDir }
    }
}

$readme = @'
# Manual Eval Coding Pack

Generated by: `manual-testing/scripts/export_eval_coding_pack.ps1`

Each folder (`C1`..`C5`) contains:
- `PROMPT.txt` (copy/paste into chat/run prompt)
- required fixture files for that task

Run from any task folder:

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "nanbeige4.1-3b@bf16" --allow-shell --allow-write --enable-write-tools --workdir . --prompt $p run
```

Recommended for C2 (prevents long hangs on verbose models):

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "essentialai/rnj-1" --allow-shell --allow-write --enable-write-tools --workdir . --max-tokens 256 --max-steps 6 --max-wall-time-ms 45000 --http-stream-idle-timeout-ms 15000 --prompt $p run
```
'@
Write-Utf8File -Path (Join-Path $outAbs "README.md") -Content $readme

Write-Host "Manual coding eval pack exported to: $outAbs"
