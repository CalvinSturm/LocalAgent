param(
    [string]$StateDir = ".tmp/manual-tui-testing/state",
    [switch]$Strict
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

$runId = [System.IO.Path]::GetFileNameWithoutExtension($latest.Name)
Write-Host ("Latest run id: " + $runId)

$args = [System.Collections.Generic.List[string]]::new()
$args.Add("run")
$args.Add("--")
$args.Add("--state-dir")
$args.Add($StateDir)
$args.Add("replay")
$args.Add("verify")
$args.Add($runId)
if ($Strict) { $args.Add("--strict") }

Write-Host ("cargo " + ($args -join " "))
cargo @args
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
