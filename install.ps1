# Instala o Estel em %LOCALAPPDATA%\Estel e inicia.
# Requer rustup (cargo no PATH) e, no alvo GNU, MinGW (gcc/as/dlltool).

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "cargo não está no PATH. Instale o Rust com rustup (https://rustup.rs) e reabra o terminal."
    exit 1
}

Write-Host "Compilando Estel (release)…"
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$src = Join-Path $PSScriptRoot "target\release\estel.exe"
if (-not (Test-Path $src)) {
    Write-Host "estel.exe não apareceu em target\release. A compilação falhou."
    exit 1
}

$destDir = Join-Path $env:LOCALAPPDATA "Estel"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
Copy-Item -Force $src (Join-Path $destDir "estel.exe")

Write-Host "Instalado em $destDir"
Write-Host "Iniciando. O ícone fica na bandeja do sistema."
Start-Process (Join-Path $destDir "estel.exe")
