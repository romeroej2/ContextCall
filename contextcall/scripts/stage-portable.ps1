# Stages the portable Windows build: copies the app executable produced by
# `tauri build` to target/release/bundle/portable/ContextCall_<version>_<arch>-portable.exe,
# then checks it is self-contained and launches it once.
#
# The frontend, icons and WebView2 loader are compiled into the exe, and the MSVC
# runtime is linked statically (see .cargo/config.toml), so the exe should import
# only DLLs that ship with Windows. Models are downloaded on first use into
# %APPDATA%\ContextCall, same as the installed app. WebView2 must be present
# (built into Windows 11 and current Windows 10).
#
# Usage: pwsh contextcall/scripts/stage-portable.ps1 [-SkipLaunch]
param(
  [switch]$SkipLaunch
)

$ErrorActionPreference = "Stop"

$workspaceRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$releaseDir = Join-Path $workspaceRoot "target\release"
$tauriConf = Join-Path $workspaceRoot "contextcall\src-tauri\tauri.conf.json"
$version = (Get-Content $tauriConf -Raw | ConvertFrom-Json).version

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
  "ARM64" { "arm64" }
  default { "x64" }
}

# tauri-cli keeps the cargo binary name (package "app") unless mainBinaryName is set.
$source = @("ContextCall.exe", "app.exe") |
  ForEach-Object { Join-Path $releaseDir $_ } |
  Where-Object { Test-Path $_ } |
  Select-Object -First 1
if (-not $source) {
  throw "No app executable found in $releaseDir. Run 'npm run tauri -- build' first."
}

$outDir = Join-Path $releaseDir "bundle\portable"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$portable = Join-Path $outDir "ContextCall_${version}_${arch}-portable.exe"
Copy-Item -Path $source -Destination $portable -Force
$sizeMb = [math]::Round((Get-Item $portable).Length / 1MB, 1)
Write-Host "Staged $portable ($sizeMb MB) from $source"

# --- Dependency check -------------------------------------------------------
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$dumpbin = $null
if (Test-Path $vswhere) {
  $dumpbin = & $vswhere -latest -products * -find "VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe" |
    Select-Object -First 1
}
if (-not $dumpbin) {
  throw "dumpbin.exe not found (needs the Visual Studio C++ build tools)."
}

$deps = & $dumpbin /nologo /dependents $portable |
  ForEach-Object { $_.Trim() } |
  Where-Object { $_ -match '^[\w\.\-]+\.dll$' } |
  Sort-Object -Unique
Write-Host "Imported DLLs:"
$deps | ForEach-Object { Write-Host "  $_" }

# Redistributable runtimes that a clean Windows install does not have.
$redist = '^(vcruntime|msvcp|vcomp|concrt|vccorlib|ucrtbased|libomp|openblas|cudart|cublas|vulkan-1)'
$system32 = Join-Path $env:SystemRoot "System32"
$bad = $deps | Where-Object {
  $_ -match $redist -or
  # api-ms-win-* are API sets resolved by the loader, not files in System32.
  (-not ($_ -match '^(api|ext)-ms-') -and -not (Test-Path (Join-Path $system32 $_)))
}
if ($bad) {
  throw "Portable exe depends on DLLs that are not part of Windows: $($bad -join ', ')"
}
Write-Host "Dependency check passed: only Windows system DLLs."

# --- Smoke launch -----------------------------------------------------------
if ($SkipLaunch) {
  return
}
Write-Host "Launching portable exe for a smoke test..."
$proc = Start-Process -FilePath $portable -PassThru
Start-Sleep -Seconds 20
if ($proc.HasExited) {
  throw "Portable exe exited after launch with code $($proc.ExitCode)."
}
Write-Host "Portable exe still running after 20 s; stopping it."
Stop-Process -Id $proc.Id -Force
