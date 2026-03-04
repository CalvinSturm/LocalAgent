param(
    [string]$StateDir = ".tmp/manual-tui-testing/state"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$runsDir = Join-Path $StateDir "runs"
if (-not (Test-Path $runsDir)) {
    throw "Runs directory not found: $runsDir"
}

$latest = Get-ChildItem -Path $runsDir -File -Filter "*.json" |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1

if (-not $latest) {
    throw "No run artifacts found in $runsDir"
}

Write-Host ("Run artifact: " + $latest.FullName)
$run = Get-Content $latest.FullName | ConvertFrom-Json

$run.cli | Select-Object `
    mode, `
    agent_mode, `
    output_mode, `
    provider, `
    model, `
    allow_shell, `
    allow_write, `
    enable_write_tools, `
    stream, `
    tui_enabled | Format-List
