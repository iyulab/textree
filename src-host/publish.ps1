param([string]$Rid = "win-x64", [string]$OutDir = "publish")
dotnet publish src/Textree.Host/Textree.Host.csproj `
  -c Release -r $Rid --self-contained true `
  -p:PublishSingleFile=true `
  -p:IncludeNativeLibrariesForSelfExtract=true `
  -p:EnableCompressionInSingleFile=true `
  -o $OutDir
