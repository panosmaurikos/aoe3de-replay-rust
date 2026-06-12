# One-command replay analysis: parse .age3Yrec -> JSON -> self-contained HTML -> open in browser.
#
#   .\analyze.ps1 "D:\AGEOFEMPIRE3TEST\testship.age3Yrec"
#   .\analyze.ps1 game.age3Yrec -DebugCommands     # also keep command-level debug data in the JSON
#   .\analyze.ps1 game.age3Yrec -NoShipments       # skip experimental shipment events
#   .\analyze.ps1 game.age3Yrec -NoBrowser         # generate files only
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$ReplayPath,
    [switch]$NoShipments,
    [switch]$DebugCommands,
    [switch]$NoBrowser
)

$ErrorActionPreference = "Stop"
$repo = $PSScriptRoot

if (-not (Test-Path -LiteralPath $ReplayPath)) {
    Write-Error "Replay not found: $ReplayPath"
}
$ReplayPath = (Resolve-Path -LiteralPath $ReplayPath).Path

$stem = [IO.Path]::GetFileNameWithoutExtension($ReplayPath)
$outDir = Join-Path $repo "target\analyze"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$jsonPath = Join-Path $outDir "$stem.json"

$flags = @()
if (-not $NoShipments) { $flags += "--experimental-shipments" }
if ($DebugCommands) { $flags += "--debug-commands" }

Write-Host "Parsing $ReplayPath ..."
& cargo run --release --quiet --manifest-path (Join-Path $repo "Cargo.toml") -- parse $ReplayPath -o $jsonPath @flags
if ($LASTEXITCODE -ne 0) {
    Write-Error "Parse failed (exit $LASTEXITCODE)"
}

# Inject the JSON into the viewer so the HTML is self-contained (no server, no file picker).
$json = [IO.File]::ReadAllText($jsonPath, [Text.Encoding]::UTF8)
$json = $json.Replace('</', '<\/')   # keep </script> inside chat strings from closing the tag
$inject = "<script>window.__AOE3_DATA__ = $json;</script>`n  "
$html = [IO.File]::ReadAllText((Join-Path $repo "viewer\index.html"), [Text.Encoding]::UTF8)
$scriptIndex = $html.IndexOf('<script>')
if ($scriptIndex -lt 0) {
    Write-Error "viewer\index.html: <script> tag not found"
}
$htmlOut = $html.Substring(0, $scriptIndex) + $inject + $html.Substring($scriptIndex)
$htmlPath = Join-Path $outDir "$stem.html"
[IO.File]::WriteAllText($htmlPath, $htmlOut, [Text.UTF8Encoding]::new($false))

Write-Host "JSON: $jsonPath"
Write-Host "HTML: $htmlPath"
if (-not $NoBrowser) {
    Start-Process $htmlPath
}
