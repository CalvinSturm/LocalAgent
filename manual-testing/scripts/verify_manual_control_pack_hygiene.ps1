param(
    [string]$Pack = "D-tests"
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path -LiteralPath ".").Path
$packRoot = Join-Path $repoRoot ("manual-testing/" + $Pack)

if (-not (Test-Path -LiteralPath $packRoot)) {
    throw "Control pack not found: $packRoot"
}

$violations = New-Object System.Collections.Generic.List[string]

$forbiddenDirNames = @(
    "target",
    ".state",
    ".localagent"
)

Get-ChildItem -LiteralPath $packRoot -Recurse -Force | ForEach-Object {
    if ($_.PSIsContainer -and ($forbiddenDirNames -contains $_.Name)) {
        $violations.Add($_.FullName)
        return
    }

    if (-not $_.PSIsContainer -and $_.Name -match '^RESULTS_.*_RUN_.*\.md$') {
        $violations.Add($_.FullName)
        return
    }
}

if ($violations.Count -gt 0) {
    Write-Error "Manual control pack hygiene check failed. Forbidden generated artifacts were found:"
    $violations | ForEach-Object { Write-Host $_ }
    exit 1
}

Write-Host "Manual control pack hygiene check passed:"
Write-Host $packRoot
