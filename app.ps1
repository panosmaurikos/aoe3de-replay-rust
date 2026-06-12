# AoE3 Replay Analyzer - tiny drop-zone UI.
# Start with "AoE3 Analyzer.cmd" (or: powershell -File app.ps1).
# Drop one or more .age3Yrec files (or click Browse); each one is parsed and
# opened in the default browser via analyze.ps1.
#
# Headless mode (no UI), used for testing and file associations:
#   powershell -File app.ps1 -RunFile "path\to\game.age3Yrec"
param(
    [string]$RunFile
)

$ErrorActionPreference = "Stop"
$repo = $PSScriptRoot

function Invoke-Analyze {
    param(
        [string]$Path,
        [bool]$DebugCommands
    )
    $analyzeArgs = @($Path)
    if ($DebugCommands) { $analyzeArgs += "-DebugCommands" }
    & (Join-Path $repo "analyze.ps1") @analyzeArgs
}

if ($RunFile) {
    Invoke-Analyze -Path $RunFile -DebugCommands $false
    return
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()

$form = New-Object System.Windows.Forms.Form
$form.Text = "AoE3 Replay Analyzer"
$form.Size = New-Object System.Drawing.Size(460, 300)
$form.MinimumSize = $form.Size
$form.StartPosition = "CenterScreen"
$form.AllowDrop = $true
$form.BackColor = [System.Drawing.Color]::FromArgb(43, 34, 24)   # dark wood
$form.ForeColor = [System.Drawing.Color]::FromArgb(232, 219, 192) # parchment

$dropLabel = New-Object System.Windows.Forms.Label
$dropLabel.Text = "Drop .age3Yrec replay here"
$dropLabel.Font = New-Object System.Drawing.Font("Segoe UI", 16, [System.Drawing.FontStyle]::Bold)
$dropLabel.TextAlign = "MiddleCenter"
$dropLabel.Dock = "Fill"
$dropLabel.AllowDrop = $true

$bottom = New-Object System.Windows.Forms.Panel
$bottom.Dock = "Bottom"
$bottom.Height = 86
$bottom.Padding = New-Object System.Windows.Forms.Padding(12)

$browseButton = New-Object System.Windows.Forms.Button
$browseButton.Text = "Browse..."
$browseButton.Width = 110
$browseButton.Height = 30
$browseButton.Left = 12
$browseButton.Top = 8
$browseButton.FlatStyle = "Flat"
$browseButton.ForeColor = $form.ForeColor

$debugCheck = New-Object System.Windows.Forms.CheckBox
$debugCheck.Text = "Include debug commands (bigger JSON, for reverse engineering)"
$debugCheck.Left = 136
$debugCheck.Top = 12
$debugCheck.Width = 300

$statusLabel = New-Object System.Windows.Forms.Label
$statusLabel.Text = "Ready."
$statusLabel.Left = 12
$statusLabel.Top = 50
$statusLabel.Width = 420
$statusLabel.ForeColor = [System.Drawing.Color]::FromArgb(170, 190, 160)

$bottom.Controls.AddRange(@($browseButton, $debugCheck, $statusLabel))
$form.Controls.Add($dropLabel)
$form.Controls.Add($bottom)

function Set-Status([string]$text, [bool]$isError = $false) {
    $statusLabel.Text = $text
    $statusLabel.ForeColor = if ($isError) {
        [System.Drawing.Color]::FromArgb(220, 120, 110)
    } else {
        [System.Drawing.Color]::FromArgb(170, 190, 160)
    }
    $form.Refresh()
}

function Start-Analysis([string[]]$paths) {
    $replays = $paths | Where-Object { $_ -match '\.age3Yrec$' }
    if (-not $replays) {
        Set-Status "Not a .age3Yrec file." $true
        return
    }
    foreach ($replay in $replays) {
        $name = [IO.Path]::GetFileName($replay)
        Set-Status "Parsing $name ... (first run compiles, takes longer)"
        try {
            Invoke-Analyze -Path $replay -DebugCommands $debugCheck.Checked
            Set-Status "Done: $name opened in browser."
        } catch {
            Set-Status "Failed: $name - $($_.Exception.Message)" $true
        }
    }
}

$dragEnter = {
    param($sender, $eventArgs)
    if ($eventArgs.Data.GetDataPresent([System.Windows.Forms.DataFormats]::FileDrop)) {
        $eventArgs.Effect = [System.Windows.Forms.DragDropEffects]::Copy
    }
}
$dragDrop = {
    param($sender, $eventArgs)
    Start-Analysis ($eventArgs.Data.GetData([System.Windows.Forms.DataFormats]::FileDrop))
}
$form.Add_DragEnter($dragEnter)
$form.Add_DragDrop($dragDrop)
$dropLabel.Add_DragEnter($dragEnter)
$dropLabel.Add_DragDrop($dragDrop)

$browseButton.Add_Click({
    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Filter = "AoE3 DE replays (*.age3Yrec)|*.age3Yrec|All files (*.*)|*.*"
    $dialog.Multiselect = $true
    $gamesDir = Join-Path $env:USERPROFILE "Games\Age of Empires 3 DE"
    if (Test-Path $gamesDir) { $dialog.InitialDirectory = $gamesDir }
    if ($dialog.ShowDialog() -eq "OK") {
        Start-Analysis $dialog.FileNames
    }
})

[void]$form.ShowDialog()
