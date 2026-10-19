# ==============================================================================
# QORDY Smart Print Bridge - Enterprise One-Line Installer (Windows PowerShell)
# Usage: irm https://raw.githubusercontent.com/ilkeraydogdu/Qordy-Kopru-Uygulamasi/main/scripts/install.ps1 | iex
# ==============================================================================
$ErrorActionPreference = "Stop"

$Repo = "ilkeraydogdu/Qordy-Kopru-Uygulamasi"
$Version = "2.5.0"
$InstallerUrl = "https://github.com/$Repo/releases/download/v$Version/Qordy_Bridge_v${Version}_Windows_Setup.exe"
$TempInstaller = "$env:TEMP\Qordy_Bridge_Setup.exe"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "  QORDY Termal Donanim Koprusu Windows Kurulumu v$Version" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

Write-Host "[1/3] Kurulum paketi indiriliyor..." -ForegroundColor Yellow
try {
    Invoke-WebRequest -Uri $InstallerUrl -OutFile $TempInstaller -UseBasicParsing
    Write-Host "[2/3] Kurulum sessiz modda baslatiliyor..." -ForegroundColor Yellow
    Start-Process -FilePath $TempInstaller -ArgumentList "/SILENT /TASKS=desktopicon,autostart" -Wait
    Remove-Item -Force $TempInstaller -ErrorAction SilentlyContinue
    Write-Host "[3/3] Kurulum basariyla tamamlandi!" -ForegroundColor Green
    Write-Host "QORDY Koprusu baslatildi. Yonetim paneli: http://localhost:8089" -ForegroundColor Green
} catch {
    Write-Host "Paket indirilemedi: $($_.Exception.Message)" -ForegroundColor Red
}
