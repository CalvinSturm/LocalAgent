param(
    [string]$Pack = "T-tests",
    [Parameter(Mandatory = $true)][string]$Task,
    [Parameter(Mandatory = $true)][string]$Model,
    [string]$Provider = "lmstudio",
    [string]$BaseUrl,
    [string]$LspProvider = "typescript",
    [string]$BinaryPath = ".\target\debug\localagent.exe",
    [string]$PromptFile = "PROMPT.txt",
    [string]$StateRoot = ".tmp/repro-state",
    [string]$TraceRoot = ".tmp/openai-traces",
    [string]$QualTraceRoot = ".tmp/qualification-traces",
    [string]$Tag,
    [bool]$AllowShell = $true,
    [bool]$AllowWrite = $true,
    [bool]$EnableWriteTools = $true,
    [bool]$NoSession = $true,
    [switch]$Stream,
    [switch]$Tui,
    [switch]$CopyPrompt,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

function Get-RepoRoot {
    (Resolve-Path -LiteralPath ".").Path
}

function Get-LatestPreparedInstanceRoot {
    param(
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][string]$PackName
    )
    $controlRoot = Join-Path $RepoRoot ".tmp/manual-testing/control/$PackName"
    $latest = Get-ChildItem -LiteralPath $controlRoot -Directory |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $latest) {
        throw "No prepared control pack instance found under $controlRoot"
    }
    $latest.FullName
}

function New-Tag {
    param(
        [Parameter(Mandatory = $true)][string]$TaskName,
        [Parameter(Mandatory = $true)][string]$ModelName,
        [Parameter(Mandatory = $true)][bool]$UseTui,
        [Parameter(Mandatory = $true)][bool]$UseStream
    )
    $slug = $ModelName.ToLowerInvariant() -replace '[^a-z0-9]+', ''
    $surface = if ($UseTui) { "tui" } else { "run" }
    $streamMode = if ($UseStream) { "stream-on" } else { "stream-off" }
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss-fff"
    "eval-{0}-{1}-{2}-{3}-{4}" -f $TaskName.ToLowerInvariant(), $slug, $surface, $streamMode, $stamp
}

function Format-CommandPreview {
    param([string[]]$CommandArgs)
    ($CommandArgs | ForEach-Object {
            if ($_ -match '\s') { '"{0}"' -f $_ } else { $_ }
        }) -join ' '
}

$repoRoot = Get-RepoRoot
Set-Location $repoRoot

pwsh -File .\manual-testing\scripts\prepare_manual_control_pack.ps1 -Pack $Pack -Task $Task | Out-Null

$instanceRoot = Get-LatestPreparedInstanceRoot -RepoRoot $repoRoot -PackName $Pack
$workdir = Join-Path $instanceRoot $Task
if (-not (Test-Path -LiteralPath $workdir)) {
    throw "Prepared task directory not found: $workdir"
}

$promptPath = Join-Path $workdir $PromptFile
if (-not (Test-Path -LiteralPath $promptPath)) {
    throw "Prompt file not found: $promptPath"
}

if ([string]::IsNullOrWhiteSpace($Tag)) {
    $Tag = New-Tag -TaskName $Task -ModelName $Model -UseTui:$Tui.IsPresent -UseStream:$Stream.IsPresent
}

$stateDir = Join-Path $repoRoot $StateRoot | Join-Path -ChildPath $Tag
$openaiTraceDir = Join-Path $repoRoot $TraceRoot | Join-Path -ChildPath $Tag
$qualTraceDir = Join-Path $repoRoot $QualTraceRoot | Join-Path -ChildPath $Tag

$env:LOCALAGENT_OPENAI_TRACE_DIR = $openaiTraceDir
$env:LOCALAGENT_QUAL_TRACE_DIR = $qualTraceDir

$prompt = Get-Content -LiteralPath $promptPath -Raw
if ($Tui.IsPresent -and $CopyPrompt.IsPresent) {
    Set-Clipboard -Value $prompt
}

$cmdArgs = @()
$cmdArgs += "--provider"
$cmdArgs += $Provider
$cmdArgs += "--model"
$cmdArgs += $Model
if (-not [string]::IsNullOrWhiteSpace($BaseUrl)) {
    $cmdArgs += "--base-url"
    $cmdArgs += $BaseUrl
}
if ($AllowShell) { $cmdArgs += "--allow-shell" }
if ($AllowWrite) { $cmdArgs += "--allow-write" }
if ($EnableWriteTools) { $cmdArgs += "--enable-write-tools" }
if (-not [string]::IsNullOrWhiteSpace($LspProvider)) {
    $cmdArgs += "--lsp-provider"
    $cmdArgs += $LspProvider
}
$cmdArgs += "--workdir"
$cmdArgs += $workdir
$cmdArgs += "--state-dir"
$cmdArgs += $stateDir
if ($NoSession) { $cmdArgs += "--no-session" }
if ($Stream) { $cmdArgs += "--stream" }

if ($Tui) {
    $cmdArgs += "chat"
    $cmdArgs += "--tui"
} else {
    $cmdArgs += "--prompt"
    $cmdArgs += $prompt
    $cmdArgs += "run"
}

Write-Host "Prepared task:"
Write-Host $workdir
Write-Host ""
Write-Host "State dir:"
Write-Host $stateDir
Write-Host ""
Write-Host "Trace dirs:"
Write-Host "  OPENAI: $openaiTraceDir"
Write-Host "  QUAL:   $qualTraceDir"
Write-Host ""
$commandPreview = Format-CommandPreview -CommandArgs $cmdArgs
if ($Tui) {
    Write-Host "Prompt file:"
    Write-Host $promptPath
    if ($CopyPrompt) {
        Write-Host "Prompt copied to clipboard."
    } else {
        Write-Host "Paste the prompt from PROMPT.txt into the TUI."
    }
    Write-Host ""
}
Write-Host "Command:"
Write-Host "$BinaryPath $commandPreview"
Write-Host ""

if (-not (Test-Path -LiteralPath $BinaryPath)) {
    throw "Binary not found: $BinaryPath"
}

if ($DryRun) {
    return
}

& $BinaryPath @cmdArgs

$runsDir = Join-Path $stateDir "runs"
if (Test-Path -LiteralPath $runsDir) {
    $latestRun = Get-ChildItem -LiteralPath $runsDir -File |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if ($latestRun) {
        Write-Output $latestRun.FullName
    }
}
