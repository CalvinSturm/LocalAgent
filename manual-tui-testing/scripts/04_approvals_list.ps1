param(
    [string]$StateDir = ".tmp/manual-tui-testing/state"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$args = @(
    "run", "--",
    "--state-dir", $StateDir,
    "approvals", "list"
)

Write-Host ("cargo " + ($args -join " "))
cargo @args
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
