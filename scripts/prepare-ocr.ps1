param([string]$CMakePath)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/ocr-common.ps1"
$repo = Split-Path -Parent $PSScriptRoot
$cache = Join-Path $repo '.tools/ocr'
$destination = Join-Path $repo 'src-tauri/resources/tesseract'
New-Item -ItemType Directory -Force $cache | Out-Null

& "$PSScriptRoot/build-ocr.ps1" -CMakePath $CMakePath
if ($LASTEXITCODE -ne 0) { throw 'Could not build the OCR runtime.' }
$model = Join-Path $cache 'eng.traineddata'
if (!(Test-Path -LiteralPath $model)) {
    Invoke-WebRequest -UseBasicParsing -Uri 'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/eng.traineddata' -OutFile $model
}
if ((Get-OcrSha256 $model) -ne '7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2') {
    throw "Checksum mismatch for $model. Remove that cached file and retry."
}

# Clear only our generated resources, including executables and DLLs from the old distribution.
if (Test-Path -LiteralPath $destination) {
    $resolved = (Resolve-Path -LiteralPath $destination).Path
    $expected = [IO.Path]::GetFullPath((Join-Path $repo 'src-tauri/resources/tesseract'))
    if ($resolved -ne $expected) { throw "Unexpected resource path: $resolved" }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
New-Item -ItemType Directory -Force "$destination/tessdata", "$destination/doc" | Out-Null
Copy-Item -LiteralPath $model -Destination "$destination/tessdata/eng.traineddata"
Copy-Item -LiteralPath "$cache/sources/tesseract-5.5.4/AUTHORS" -Destination "$destination/doc/Tesseract-AUTHORS.txt"
Copy-Item -LiteralPath "$PSScriptRoot/ocr-NOTICE.txt" -Destination "$destination/NOTICE.txt"

# Copy licenses from the exact hash-verified source archives used for this build.
$packages = Get-Content -LiteralPath "$PSScriptRoot/ocr-sources.json" -Raw | ConvertFrom-Json
$manifest = @{
    components = @($packages | ForEach-Object { $_ })
    englishModel = @{
        version = 'tessdata_fast 4.1.0'
        url = 'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/eng.traineddata'
        sha256 = Get-OcrSha256 $model
        license = 'Apache-2.0; see doc/LICENSE-tesseract.txt'
    }
    nativeLibraries = @($packages | ForEach-Object { @{ name = "$($_.library).lib"; sha256 = Get-OcrSha256 "$cache/native/install/lib/$($_.library).lib" } })
    recipeSha256 = Get-OcrSha256 "$PSScriptRoot/build-ocr.ps1"
    hashHelperSha256 = Get-OcrSha256 "$PSScriptRoot/ocr-common.ps1"
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$destination/SOURCE-MANIFEST.json" -Encoding UTF8
foreach ($package in $packages) {
    $license = Join-Path (Join-Path "$cache/sources" $package.directory) $package.license
    Copy-Item -LiteralPath $license -Destination "$destination/doc/LICENSE-$($package.name).txt"
}

# Native recognition is exercised by cargo test after these resources are staged.
Write-Host 'Native OCR libraries and bundled English model prepared. Run cargo test in src-tauri to verify recognition.'
