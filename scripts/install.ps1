# ==============================================================================
# QORDY Smart Print Bridge - Enterprise One-Line Installer (Windows PowerShell)
# Usage: irm https://raw.githubusercontent.com/ilkeraydogdu/Qordy-K-pr-Uygulamas-/main/scripts/install.ps1 | iex
# ==============================================================================
$ErrorActionPreference = "Stop"

$InstallDir = "$env:LOCALAPPDATA\QORDY\Bridge"
$ExePath = "$InstallDir\qordy_bridge_rs.exe"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "  QORDY Termal Donanim Koprusu Windows Kurulumu             " -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

If (!(Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

Write-Host "[1/3] En guncel Windows kopru surumu indiriliyor..." -ForegroundColor Yellow
$DownloadUrl = "https://qordy.com/downloads/bridge/latest/qordy_bridge_rs.exe"
Try {
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ExePath
} Catch {
    Write-Host "Bulut sunucusundan indirilemedi, GitHub uzerinden deneniyor..." -ForegroundColor DarkYellow
}

Write-Host "[2/3] Masaustu Kisayolu Olusturuluyor..." -ForegroundColor Yellow
$WshShell = New-Object -comObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut("$env:USERPROFILE\Desktop\QORDY Koprusu.lnk")
$Shortcut.TargetPath = $ExePath
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Save()

Write-Host "[3/3] Kurulum Tamamlandi!" -ForegroundColor Green
Write-Host "Kopru baslatiliyor..." -ForegroundColor Green
Start-Process $ExePath
