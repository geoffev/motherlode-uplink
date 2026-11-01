param([string]$CMakePath)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/ocr-common.ps1"
$repo = Split-Path -Parent $PSScriptRoot
$cache = Join-Path $repo '.tools/ocr'
$sources = Join-Path $cache 'sources'
$native = Join-Path $cache 'native'
$prefix = Join-Path $native 'install'
New-Item -ItemType Directory -Force $sources, $native | Out-Null

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (!(Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio C++ Build Tools are required.' }
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json | ConvertFrom-Json
if (!$vs) { throw 'Install Visual Studio C++ Build Tools with the Windows SDK.' }
$vs = @($vs)[0]
if (!$CMakePath) {
    $command = Get-Command cmake.exe -ErrorAction SilentlyContinue
    if ($command) { $CMakePath = $command.Source }
    else { $CMakePath = Join-Path $vs.installationPath 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe' }
}
if (!(Test-Path -LiteralPath $CMakePath)) { throw 'Install CMake or pass -CMakePath pointing to cmake.exe.' }
$generator = if ([version]$vs.installationVersion -ge [version]'18.0') { 'Visual Studio 18 2026' } else { 'Visual Studio 17 2022' }

function Invoke-CMake([string[]]$Arguments) {
    # Windows PowerShell treats native stderr warnings as PowerShell errors.
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $CMakePath @Arguments >> "$native/build.log" 2>&1
    } finally { $ErrorActionPreference = $previousPreference }
    if ($LASTEXITCODE -ne 0) {
        Get-Content -LiteralPath "$native/build.log" -Tail 60 | Write-Host
        throw "OCR native build failed. See $native/build.log"
    }
}

$packages = Get-Content -LiteralPath "$PSScriptRoot/ocr-sources.json" -Raw | ConvertFrom-Json
foreach ($package in $packages) {
    $archive = Join-Path $sources "$($package.name).tar.gz"
    if (!(Test-Path -LiteralPath $archive)) {
        Invoke-WebRequest -UseBasicParsing -Uri $package.url -OutFile $archive
    }
    if ((Get-OcrSha256 $archive) -ne $package.sha256) {
        throw "Checksum mismatch for $archive. Remove that cached file and retry."
    }
    if (!(Test-Path -LiteralPath (Join-Path $sources $package.directory))) {
        & tar.exe -xf $archive -C $sources
        if ($LASTEXITCODE -ne 0) { throw "Could not extract $archive" }
    }
}

# Rebuild when the recipe, source pins or selected compiler installation changes.
$key = ((Get-OcrSha256 $PSCommandPath) + (Get-OcrSha256 "$PSScriptRoot/ocr-sources.json") + (Get-OcrSha256 "$PSScriptRoot/ocr-common.ps1") + $vs.installationVersion + $vs.installationPath)
$stamp = Join-Path $native 'recipe.txt'
if ((Test-Path -LiteralPath "$prefix/bin/tesseract.exe") -and (Test-Path -LiteralPath $stamp) -and (Get-Content -LiteralPath $stamp -Raw).Trim() -eq $key) {
    Write-Host 'Using cached minimal OCR build.'
    exit 0
}

$common = @('-G', $generator, '-A', 'x64', "-DCMAKE_GENERATOR_INSTANCE=$($vs.installationPath)",
    "-DCMAKE_INSTALL_PREFIX=$prefix", "-DCMAKE_PREFIX_PATH=$prefix", '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded',
    '-DCMAKE_POLICY_DEFAULT_CMP0091=NEW', '-DCMAKE_POLICY_VERSION_MINIMUM=3.10', '-DBUILD_SHARED_LIBS=OFF',
    '-DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF', '-DCMAKE_FIND_USE_SYSTEM_PACKAGE_REGISTRY=OFF')
$options = @{
    zlib = @('-DZLIB_BUILD_SHARED=OFF', '-DZLIB_BUILD_STATIC=ON', '-DZLIB_BUILD_TESTING=OFF')
    libpng = @('-DPNG_SHARED=OFF', '-DPNG_STATIC=ON', '-DPNG_TESTS=OFF', '-DPNG_TOOLS=OFF', "-DZLIB_LIBRARY=$prefix/lib/zs.lib")
    leptonica = @('-DSW_BUILD=OFF', '-DBUILD_PROG=OFF', '-DSTRICT_CONF=ON', '-DENABLE_ZLIB=ON', '-DENABLE_PNG=ON',
        '-DENABLE_GIF=OFF', '-DENABLE_JPEG=OFF', '-DENABLE_TIFF=OFF', '-DENABLE_WEBP=OFF', '-DENABLE_OPENJPEG=OFF', "-DZLIB_LIBRARY=$prefix/lib/zs.lib")
    tesseract = @('-DWIN32_MT_BUILD=ON', '-DBUILD_TRAINING_TOOLS=OFF', '-DBUILD_TESTS=OFF', '-DOPENMP_BUILD=OFF',
        '-DGRAPHICS_DISABLED=ON', '-DDISABLE_ARCHIVE=ON', '-DDISABLE_CURL=ON', '-DDISABLE_TIFF=ON', '-DENABLE_NATIVE=OFF',
        '-DCMAKE_CXX_FLAGS=-DTESSERACT_DISABLE_DEBUG_FONTS')
}
foreach ($package in $packages) {
    Write-Host "Building $($package.name) $($package.version)..."
    $build = Join-Path $native $package.name
    Invoke-CMake (@('-S', (Join-Path $sources $package.directory), '-B', $build) + $common + $options[$package.name])
    Invoke-CMake @('--build', $build, '--config', 'Release', '--parallel', '4')
    Invoke-CMake @('--install', $build, '--config', 'Release')
}
Set-Content -LiteralPath $stamp -Value $key
