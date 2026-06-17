#!/usr/bin/env pwsh
# Authenticode-signs a single Windows PE file via AzureSignTool + Azure Key Vault (GlobalSign EV).
# Invoked by Tauri's bundle.windows.signCommand during `tauri build`; receives the file path as $args[0].
# KV access token and parameters come from the environment (set by the release workflow) because
# signCommand is a static string and cannot carry the dynamic token.
$ErrorActionPreference = 'Stop'

$file = $args[0]
if (-not $file) { throw 'sign-windows.ps1: no file path argument provided' }

foreach ($name in 'AZURE_KV_TOKEN', 'AZURE_KV_URL', 'AZURE_KV_CERT', 'AZURE_TIMESTAMP_URL') {
    if (-not (Get-Item "env:$name" -ErrorAction SilentlyContinue)) {
        throw "sign-windows.ps1: required env var $name is not set"
    }
}

AzureSignTool sign `
    --description-url 'https://github.com/iyulab/textree' `
    --azure-key-vault-url $env:AZURE_KV_URL `
    --azure-key-vault-accesstoken $env:AZURE_KV_TOKEN `
    --azure-key-vault-certificate $env:AZURE_KV_CERT `
    --timestamp-rfc3161 $env:AZURE_TIMESTAMP_URL `
    --timestamp-digest sha256 `
    --file-digest sha256 `
    --verbose `
    $file

if ($LASTEXITCODE -ne 0) { throw "AzureSignTool failed with exit code $LASTEXITCODE" }
