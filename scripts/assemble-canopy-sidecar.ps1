#Requires -Version 7
# Assembles the canopy renderer as a self-contained "mechanism B" sidecar payload:
# a pinned Node runtime + canopy's built dist + production node_modules. The payload is
# bundled by Tauri as a resource (see tauri.conf.json) and invoked as `node cli.js` at
# runtime (see resolve_canopy). Idempotent: safe to re-run. Windows-first (win-x64).
[CmdletBinding()]
param(
  # Path to the canopy source repo. Default = umbrella sibling. CI passes the checked-out copy.
  [string]$CanopyPath = (Join-Path $PSScriptRoot '..' '..' 'canopy'),
  [string]$NodeVersion = '22.12.0'
)
$ErrorActionPreference = 'Stop'

$repoRoot  = Join-Path $PSScriptRoot '..'                       # textree/
$stage     = Join-Path $repoRoot 'src-tauri' 'resources' 'canopy'
$cacheDir  = Join-Path $repoRoot '.cache'
$canopy    = (Resolve-Path $CanopyPath).Path

Write-Host "Assembling canopy sidecar: node v$NodeVersion + $canopy -> $stage"

# 1. Build canopy from source (produces dist/). Needs dev deps for tsc.
Push-Location $canopy
try {
  npm ci
  npm run build
} finally { Pop-Location }

# 2. Reset the stage dir.
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

# 3. Copy built dist/* + the package manifests (package.json carries "type":"module",
#    which makes node treat the .js files as ESM; package-lock pins the prod install).
Copy-Item (Join-Path $canopy 'dist' '*') $stage -Recurse -Force
Copy-Item (Join-Path $canopy 'package.json') $stage -Force
Copy-Item (Join-Path $canopy 'package-lock.json') $stage -Force

# 4. Production-only install INTO the stage (does not mutate the source canopy node_modules).
Push-Location $stage
try { npm ci --omit=dev } finally { Pop-Location }

# 5. Fetch + cache the pinned Node runtime, extract node.exe into the stage.
$nodeExe = Join-Path $stage 'node.exe'
if (-not (Test-Path $nodeExe)) {
  New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
  $zipName = "node-v$NodeVersion-win-x64"
  $zip     = Join-Path $cacheDir "$zipName.zip"
  if (-not (Test-Path $zip)) {
    Invoke-WebRequest "https://nodejs.org/dist/v$NodeVersion/$zipName.zip" -OutFile $zip
  }
  $extract = Join-Path $cacheDir $zipName
  if (-not (Test-Path $extract)) { Expand-Archive $zip -DestinationPath $cacheDir -Force }
  Copy-Item (Join-Path $extract 'node.exe') $nodeExe -Force
}

Write-Host "Done. Payload at $stage"
