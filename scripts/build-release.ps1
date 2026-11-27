param(
    [string]$Out = "dist"
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

New-Item -ItemType Directory -Force -Path $Out | Out-Null
Copy-Item "target\release\safeusb.exe" (Join-Path $Out "safeusb.exe") -Force
Write-Host "built $Out\safeusb.exe"
