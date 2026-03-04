param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("lmstudio", "ollama", "llamacpp", "mock")]
    [string]$Provider,

    [Parameter(Mandatory = $true)]
    [string]$Model,

    [ValidateSet("build", "plan")]
    [string]$AgentMode = "plan",

    [string]$Workdir = ".tmp/manual-tui-testing/workdir",
    [string]$StateDir = ".tmp/manual-tui-testing/state",

    [ValidateSet("interrupt", "fail")]
    [string]$ApprovalMode = "interrupt",

    [switch]$AllowShell,
    [switch]$EnableWrite,
    [switch]$PlainTui,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$args = [System.Collections.Generic.List[string]]::new()
$args.Add("run")
$args.Add("--")
$args.Add("--provider")
$args.Add($Provider)
$args.Add("--model")
$args.Add($Model)
$args.Add("--workdir")
$args.Add($Workdir)
$args.Add("--state-dir")
$args.Add($StateDir)
$args.Add("--trust")
$args.Add("on")
$args.Add("--approval-mode")
$args.Add($ApprovalMode)
$args.Add("--agent-mode")
$args.Add($AgentMode)

if ($EnableWrite) {
    $args.Add("--enable-write-tools")
    $args.Add("--allow-write")
}
if ($AllowShell) {
    $args.Add("--allow-shell")
}

$args.Add("chat")
if ($PlainTui) { $args.Add("--plain-tui") } else { $args.Add("--tui") }

$preview = "cargo " + ($args -join " ")
Write-Host $preview
if ($DryRun) { return }

cargo @args
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
