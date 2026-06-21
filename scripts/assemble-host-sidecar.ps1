#requires -Version 7
# Assembles the local-AI host sidecar (mechanism B — parallel to assemble-canopy-sidecar.ps1).
# Publishes the .NET host as a self-contained single-file win-x64 exe and stages it under
# src-tauri/resources/host/ so tauri-action bundles it into the installer. Idempotent.
param([string]$Rid = "win-x64")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot            # repo root (scripts/ sits at the repo root)
$proj = Join-Path $root "src-host/src/Textree.Host/Textree.Host.csproj"
$stage = Join-Path $root "src-tauri/resources/host"
$publish = Join-Path $root ".cache/host-publish"

# Self-contained single-file: native libs (ONNX runtime, SQLite) self-extract at launch, so the
# target needs no .NET runtime and no DOTNET_ROOT.
& dotnet publish $proj `
  -c Release -r $Rid --self-contained true `
  -p:PublishSingleFile=true `
  -p:IncludeNativeLibrariesForSelfExtract=true `
  -p:EnableCompressionInSingleFile=true `
  -o $publish
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item (Join-Path $publish "textree-host.exe") (Join-Path $stage "textree-host.exe")
Write-Host "host sidecar assembled -> $stage/textree-host.exe"
