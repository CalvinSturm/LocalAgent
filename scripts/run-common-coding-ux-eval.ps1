param(
    [Parameter(Mandatory = $true)]
    [string]$Model,

    [ValidateSet("lmstudio", "ollama", "llamacpp", "mock")]
    [string]$Provider = "lmstudio",

    [string]$BaseUrl = "http://localhost:1234/v1",

    [string]$StateDir = ".localagent-benchmark-state",

    [string]$OutRoot = ".artifacts/eval/common_coding_ux",

    [int]$RunsPerTask = 1,

    [string]$BaselineName,

    [string]$CompareBaseline,

    [string]$Label,

    [switch]$DryRun,

    [string[]]$ExtraArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Sanitize-Slug {
    param([string]$Value)

    $slug = $Value.ToLowerInvariant()
    $slug = [System.Text.RegularExpressions.Regex]::Replace($slug, "[^a-z0-9]+", "_")
    $slug = $slug.Trim("_")
    if ([string]::IsNullOrWhiteSpace($slug)) {
        throw "Cannot derive a slug from the provided value."
    }

    return $slug
}

function Resolve-LocalAgentCommand {
    $command = Get-Command "localagent" -ErrorAction SilentlyContinue
    if ($command) {
        return @("localagent")
    }

    return @("cargo", "run", "--")
}

$dateStamp = Get-Date -Format "yyyy-MM-dd"
$modelSlug = Sanitize-Slug -Value $Model
$runLabel = if ($Label) { $Label } else { "$modelSlug-$dateStamp" }
$runDir = Join-Path $OutRoot $runLabel
$resultsPath = Join-Path $runDir "run.json"
$summaryPath = Join-Path $runDir "SUMMARY.md"
$junitPath = Join-Path $runDir "junit.xml"
$bundlePath = Join-Path $runDir "bundle.zip"

$argsList = [System.Collections.Generic.List[string]]::new()
$argsList.Add("eval")
$argsList.Add("--provider")
$argsList.Add($Provider)
$argsList.Add("--base-url")
$argsList.Add($BaseUrl)
$argsList.Add("--models")
$argsList.Add($Model)
$argsList.Add("--pack")
$argsList.Add("common_coding_ux")
$argsList.Add("--runs-per-task")
$argsList.Add($RunsPerTask.ToString())
$argsList.Add("--allow-write")
$argsList.Add("--allow-shell")
$argsList.Add("--enable-write-tools")
$argsList.Add("--state-dir")
$argsList.Add($StateDir)
$argsList.Add("--out")
$argsList.Add($resultsPath)
$argsList.Add("--summary-md")
$argsList.Add($summaryPath)
$argsList.Add("--junit")
$argsList.Add($junitPath)
$argsList.Add("--bundle")
$argsList.Add($bundlePath)

if ($CompareBaseline) {
    $argsList.Add("--compare-baseline")
    $argsList.Add($CompareBaseline)
    $argsList.Add("--fail-on-regression")
}
else {
    $resolvedBaselineName = if ($BaselineName) {
        $BaselineName
    }
    else {
        "broad_common_coding_ux_{0}_{1}" -f $modelSlug, ($dateStamp -replace "-", "_")
    }

    $argsList.Add("--baseline")
    $argsList.Add($resolvedBaselineName)
}

if ($ExtraArgs) {
    foreach ($arg in $ExtraArgs) {
        $argsList.Add($arg)
    }
}

$commandPrefix = Resolve-LocalAgentCommand
$previewParts = [System.Collections.Generic.List[string]]::new()
foreach ($part in $commandPrefix) {
    $previewParts.Add($part)
}
foreach ($part in $argsList) {
    $previewParts.Add($part)
}
$preview = $previewParts -join " "
Write-Host $preview

if ($DryRun) {
    return
}

New-Item -ItemType Directory -Force -Path $runDir | Out-Null
New-Item -ItemType Directory -Force -Path $StateDir | Out-Null

if ($commandPrefix.Count -eq 1) {
    & $commandPrefix[0] @argsList
}
else {
    & $commandPrefix[0] $commandPrefix[1] $commandPrefix[2] @argsList
}

$exit = $LASTEXITCODE
if ($null -ne $exit -and $exit -ne 0) {
    exit $exit
}
