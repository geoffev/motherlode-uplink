param([string]$CMakePath)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$cache = Join-Path $repo '.tools/ocr'
$destination = Join-Path $repo 'src-tauri/resources/tesseract'
New-Item -ItemType Directory -Force $cache | Out-Null

& "$PSScriptRoot/build-ocr.ps1" -CMakePath $CMakePath
if ($LASTEXITCODE -ne 0) { throw 'Could not build the OCR runtime.' }
$executable = Join-Path $cache 'native/install/bin/tesseract.exe'
& "$PSScriptRoot/check-ocr-imports.ps1" -Executable $executable

$model = Join-Path $cache 'eng.traineddata'
if (!(Test-Path -LiteralPath $model)) {
    Invoke-WebRequest -UseBasicParsing -Uri 'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/eng.traineddata' -OutFile $model
}
if ((Get-FileHash -LiteralPath $model -Algorithm SHA256).Hash -ne '7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2') {
    throw "Checksum mismatch for $model. Remove that cached file and retry."
}

# Clear only our generated resources, including DLLs from the old distribution.
if (Test-Path -LiteralPath $destination) {
    $resolved = (Resolve-Path -LiteralPath $destination).Path
    $expected = [IO.Path]::GetFullPath((Join-Path $repo 'src-tauri/resources/tesseract'))
    if ($resolved -ne $expected) { throw "Unexpected resource path: $resolved" }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
New-Item -ItemType Directory -Force "$destination/tessdata/configs", "$destination/doc" | Out-Null
Copy-Item -LiteralPath $executable -Destination "$destination/tesseract.exe"
Copy-Item -LiteralPath $model -Destination "$destination/tessdata/eng.traineddata"
Copy-Item -LiteralPath "$cache/sources/tesseract-5.5.3/tessdata/configs/tsv" -Destination "$destination/tessdata/configs/tsv"
Copy-Item -LiteralPath "$cache/sources/tesseract-5.5.3/AUTHORS" -Destination "$destination/doc/Tesseract-AUTHORS.txt"
Copy-Item -LiteralPath "$PSScriptRoot/ocr-NOTICE.txt" -Destination "$destination/NOTICE.txt"

# Copy licenses from the exact hash-verified source archives used for this build.
$packages = Get-Content -LiteralPath "$PSScriptRoot/ocr-sources.json" -Raw | ConvertFrom-Json
$manifest = @{
    components = @($packages | ForEach-Object { $_ })
    englishModel = @{
        version = 'tessdata_fast 4.1.0'
        url = 'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/eng.traineddata'
        sha256 = (Get-FileHash -LiteralPath $model -Algorithm SHA256).Hash.ToLowerInvariant()
        license = 'Apache-2.0; see doc/LICENSE-tesseract.txt'
    }
    executableSha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
    recipeSha256 = (Get-FileHash -LiteralPath "$PSScriptRoot/build-ocr.ps1" -Algorithm SHA256).Hash.ToLowerInvariant()
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$destination/SOURCE-MANIFEST.json" -Encoding UTF8
foreach ($package in $packages) {
    $license = Join-Path (Join-Path "$cache/sources" $package.directory) $package.license
    Copy-Item -LiteralPath $license -Destination "$destination/doc/LICENSE-$($package.name).txt"
}

# Exercise the staged files, not an existing system installation. Poison the
# data environment and strip PATH to catch accidental external dependencies.
Add-Type -AssemblyName System.Drawing
$imagePath = Join-Path $cache 'smoke.png'
$bitmap = New-Object System.Drawing.Bitmap 700, 100
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$font = New-Object System.Drawing.Font 'Arial', 36
try {
    $graphics.Clear([System.Drawing.Color]::White)
    $graphics.DrawString('HELLO 12345', $font, [System.Drawing.Brushes]::Black, 10, 10)
    $bitmap.Save($imagePath, [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $font.Dispose()
    $graphics.Dispose()
    $bitmap.Dispose()
}
$previousPath = $env:PATH
$previousData = $env:TESSDATA_PREFIX
New-Item -ItemType Directory -Force (Join-Path $cache 'not-a-model-directory') | Out-Null
try {
    $env:PATH = "$env:SystemRoot/system32;$env:SystemRoot"
    $env:TESSDATA_PREFIX = Join-Path $cache 'not-a-model-directory'
    $result = & "$destination/tesseract.exe" $imagePath stdout --tessdata-dir "$destination/tessdata" -l eng --psm 7 tsv
    if ($LASTEXITCODE -ne 0 -or ($result -join "`n") -notmatch 'HELLO') {
        throw 'Bundled Tesseract failed its English TSV smoke test.'
    }
} finally {
    $env:PATH = $previousPath
    $env:TESSDATA_PREFIX = $previousData
}
Write-Host 'Bundled Tesseract 5.5.3: English TSV OCR smoke test passed.'
