param(
    [Parameter(Mandatory = $true)][string]$ResultsFile,
    [Parameter(Mandatory = $true)][string]$Date,
    [Parameter(Mandatory = $true)][string]$Provider,
    [Parameter(Mandatory = $true)][string]$Model,
    [Parameter(Mandatory = $true)][string]$Task,
    [Parameter(Mandatory = $true)][string]$RunId,
    [Parameter(Mandatory = $true)][string]$ArtifactPath,
    [Parameter(Mandatory = $true)][string]$PreparedInstanceId,
    [Parameter(Mandatory = $true)][string]$SessionMode,
    [Parameter(Mandatory = $true)][string]$ExitReason,
    [Parameter(Mandatory = $true)][string]$ReasonCode,
    [Parameter(Mandatory = $true)][string]$FailureClass,
    [Parameter(Mandatory = $true)][string]$TaskResult,
    [Parameter(Mandatory = $true)][string]$FinalAnswer,
    [Parameter(Mandatory = $true)][string]$FileResult,
    [Parameter(Mandatory = $true)][string]$TestResult,
    [Parameter(Mandatory = $true)][string]$PathPreexisted,
    [Parameter(Mandatory = $true)][string]$ToolCallsTotal,
    [Parameter(Mandatory = $true)][string]$BytesWritten,
    [Parameter(Mandatory = $true)][string]$ExecutionQuality,
    [Parameter(Mandatory = $true)][string]$FailureLayer,
    [Parameter(Mandatory = $true)][string]$Notes
)

$ErrorActionPreference = "Stop"

function Escape-Cell {
    param([string]$Text)
    if ($null -eq $Text) { return "" }
    return ($Text -replace '\|', '\|' -replace "`r?`n", '<br>')
}

$row = @(
    $Date,
    $Provider,
    $Model,
    $Task,
    "`"$(Escape-Cell $RunId)`"",
    "`"$(Escape-Cell $ArtifactPath)`"",
    "`"$(Escape-Cell $PreparedInstanceId)`"",
    $SessionMode,
    "`"$(Escape-Cell $ExitReason)`"",
    "`"$(Escape-Cell $ReasonCode)`"",
    "`"$(Escape-Cell $FailureClass)`"",
    $TaskResult,
    "`"$(Escape-Cell $FinalAnswer)`"",
    $FileResult,
    $TestResult,
    $PathPreexisted,
    $ToolCallsTotal,
    $BytesWritten,
    $ExecutionQuality,
    $FailureLayer,
    (Escape-Cell $Notes)
) -join " | "

$line = "| $row |"
$parent = Split-Path -Parent $ResultsFile
if ($parent -and -not (Test-Path -LiteralPath $parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
}
Add-Content -LiteralPath $ResultsFile -Value $line
Write-Host "Appended result row to:"
Write-Host $ResultsFile
