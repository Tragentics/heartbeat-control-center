# Tragentics Heartbeat Control Center — full verification pipeline.
# Runs every quality gate in order and stops on the first failure.
# Usage:  pwsh -File scripts\verify.ps1            (from "Desktop Application")
#         pwsh -File scripts\verify.ps1 -Bundle    (also builds the NSIS installer)

param(
    [switch]$Bundle
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$app = Join-Path $root 'heartbeat-control-center'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }

$gates = @()
function Run-Gate([string]$Name, [scriptblock]$Body) {
    Write-Host ""
    Write-Host "== $Name ==" -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $Body
    if ($LASTEXITCODE -ne 0) {
        Write-Host "GATE FAILED: $Name (exit $LASTEXITCODE)" -ForegroundColor Red
        exit 1
    }
    $sw.Stop()
    $script:gates += "{0}  ({1:n1}s)" -f $Name, $sw.Elapsed.TotalSeconds
    Write-Host "PASS: $Name" -ForegroundColor Green
}

Set-Location $app

Run-Gate 'assets: icons generated' {
    node scripts/generate-icons.mjs
}
Run-Gate 'assets: fonts vendored' {
    node scripts/vendor-fonts.mjs
}
Run-Gate 'frontend: tsc --noEmit' {
    npx tsc --noEmit
}
Run-Gate 'frontend: vite build' {
    npx vite build
}

Set-Location (Join-Path $app 'src-tauri')

Run-Gate 'rust: cargo fmt --check' {
    cargo fmt --check
}
Run-Gate 'rust: cargo clippy (deny warnings)' {
    cargo clippy --all-targets -- -D warnings
}
Run-Gate 'rust: cargo test' {
    cargo test
}

if ($Bundle) {
    Set-Location $app
    Run-Gate 'bundle: tauri build (NSIS installer)' {
        npx tauri build
    }
}

Write-Host ""
Write-Host "ALL GATES PASSED" -ForegroundColor Green
$gates | ForEach-Object { Write-Host "  ✓ $_" }
