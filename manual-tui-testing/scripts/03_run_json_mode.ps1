param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("lmstudio", "ollama", "llamacpp", "mock")]
    [string]$Provider,

    [Parameter(Mandatory = $true)]
    [string]$Model,

    [Parameter(Mandatory = $true)]
    [string]$Prompt,

    [ValidateSet("build", "plan")]
    [string]$AgentMode = "plan",

    [string]$Workdir = ".tmp/manual-tui-testing/workdir",
    [string]$StateDir = ".tmp/manual-tui-testing/state",
    [string]$OutJsonl = ".tmp/manual-tui-testing/session/latest_run_events.jsonl",
    [switch]$Stream
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$outDir = Split-Path -Parent $OutJsonl
if (-not [string]::IsNullOrWhiteSpace($outDir)) {
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
}

$args = [System.Collections.Generic.List[string]]::new()
$args.Add("run")
$args.Add("--")
$args.Add("--provider"); $args.Add($Provider)
$args.Add("--model"); $args.Add($Model)
$args.Add("--workdir"); $args.Add($Workdir)
$args.Add("--state-dir"); $args.Add($StateDir)
$args.Add("--trust"); $args.Add("on")
$args.Add("--agent-mode"); $args.Add($AgentMode)
$args.Add("--output"); $args.Add("json")
$args.Add("--no-session")
if ($Stream) { $args.Add("--stream") }
$args.Add("--prompt"); $args.Add($Prompt)
$args.Add("run")

$preview = "cargo " + ($args -join " ")
Write-Host $preview

$jsonl = cargo @args
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$jsonl | Set-Content $OutJsonl
Write-Host ("Wrote JSONL stdout to: " + (Resolve-Path $OutJsonl))

try {
    $rows = Get-Content $OutJsonl | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object { $_ | ConvertFrom-Json }
    if ($rows.Count -eq 0) {
        Write-Host "WARN: no JSON rows emitted"
        return
    }
    $last = $rows[-1]
    Write-Host ("Rows: " + $rows.Count)
    Write-Host ("First type: " + $rows[0].type)
    Write-Host ("Last type: " + $last.type)
    Write-Host ("Last run_id: " + $last.run_id)
} catch {
    Write-Host "WARN: failed to parse JSONL output"
}
