param(
    [string]$Pack = "D-tests",
    [string]$Task,
    [string]$OutRoot = ".tmp/manual-testing/control"
)

$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    return (Resolve-Path -LiteralPath ".").Path
}

function Reset-Dir {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
}

function Copy-IfExists {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    if (Test-Path -LiteralPath $Source) {
        Copy-Item -LiteralPath $Source -Destination $Destination -Recurse -Force
    }
}

$repoRoot = Resolve-RepoRoot
$sourcePack = Join-Path $repoRoot ("manual-testing/" + $Pack)

if (-not (Test-Path -LiteralPath $sourcePack)) {
    throw "Control pack not found: $sourcePack"
}

$instanceId = "{0}-{1}" -f (Get-Date -Format "yyyyMMdd-HHmmss-fff"), ([guid]::NewGuid().ToString("N").Substring(0, 6))
$instanceRoot = Join-Path (Join-Path (Join-Path $repoRoot $OutRoot) $Pack) $instanceId
Reset-Dir -Path $instanceRoot

$preparedAt = (Get-Date).ToString("o")

$readme = Join-Path $sourcePack "README.md"
Copy-IfExists -Source $readme -Destination $instanceRoot

$resultsDir = Join-Path $instanceRoot "results"
New-Item -ItemType Directory -Force -Path $resultsDir | Out-Null

$templatePath = Join-Path (Join-Path $sourcePack "results") "RESULTS_TEMPLATE_D.md"
Copy-IfExists -Source $templatePath -Destination $resultsDir

if ($Task) {
    $taskSource = Join-Path $sourcePack $Task
    if (-not (Test-Path -LiteralPath $taskSource)) {
        throw "Task '$Task' not found in pack '$Pack'"
    }
    Copy-Item -LiteralPath $taskSource -Destination $instanceRoot -Recurse -Force
} else {
    Get-ChildItem -LiteralPath $sourcePack -Directory |
        Where-Object { $_.Name -match '^[A-Z]\d+$' } |
        ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination $instanceRoot -Recurse -Force
        }
}

$manifest = [ordered]@{
    schema_version = "localagent.manual_control_instance.v1"
    pack = $Pack
    task = $Task
    source_pack = $sourcePack
    instance_id = $instanceId
    prepared_at = $preparedAt
    prepared_root = $instanceRoot
}

$manifestPath = Join-Path $instanceRoot "PREPARED_INSTANCE.json"
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath -Encoding utf8

Write-Host "Prepared control pack instance:"
Write-Host $instanceRoot

if ($Task) {
    Write-Host ""
    Write-Host "Run from:"
    Write-Host (Join-Path $instanceRoot $Task)
}
