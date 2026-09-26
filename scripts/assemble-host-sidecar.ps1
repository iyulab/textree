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
# The host reports its version in diagnostics; without this it would say the SDK default (1.0.0)
# in every release. The app's version is the single source.
$version = (Get-Content (Join-Path $root "src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json).version
if (-not $version) { throw "no version in src-tauri/tauri.conf.json" }

# Self-contained single-file: native libs (ONNX runtime, SQLite) self-extract at launch, so the
# target needs no .NET runtime and no DOTNET_ROOT.
& dotnet publish $proj `
  -c Release -r $Rid --self-contained true `
  -p:Version=$version `
  -p:PublishSingleFile=true `
  -p:IncludeNativeLibrariesForSelfExtract=true `
  -p:EnableCompressionInSingleFile=true `
  -o $publish
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item (Join-Path $publish "textree-host.exe") (Join-Path $stage "textree-host.exe")
# Record which source this build came from, so `host:smoke --exe` can tell a stale build apart.
& node (Join-Path $PSScriptRoot "sidecar-provenance.mjs") stamp host
if ($LASTEXITCODE -ne 0) { throw "recording the sidecar's source failed ($LASTEXITCODE)" }
Write-Host "host sidecar $version assembled -> $stage/textree-host.exe"
