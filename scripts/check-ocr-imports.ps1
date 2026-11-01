param([Parameter(Mandatory = $true)][string]$Executable)

$ErrorActionPreference = 'Stop'
$bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Executable).Path)
$pe = [BitConverter]::ToInt32($bytes, 0x3c)
if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x8664) { throw 'Expected an x64 OCR executable.' }
$optional = $pe + 24
if ([BitConverter]::ToUInt16($bytes, $optional) -ne 0x20b) { throw 'Expected a PE32+ OCR executable.' }
$count = [BitConverter]::ToUInt16($bytes, $pe + 6)
$sectionStart = $optional + [BitConverter]::ToUInt16($bytes, $pe + 20)
function Get-FileOffset([uint32]$Rva) {
    for ($i = 0; $i -lt $count; $i++) {
        $section = $sectionStart + $i * 40
        $size = [Math]::Max([BitConverter]::ToUInt32($bytes, $section + 8), [BitConverter]::ToUInt32($bytes, $section + 16))
        $address = [BitConverter]::ToUInt32($bytes, $section + 12)
        if ($Rva -ge $address -and $Rva -lt $address + $size) {
            return [int]([BitConverter]::ToUInt32($bytes, $section + 20) + $Rva - $address)
        }
    }
    throw "Invalid PE address: $Rva"
}
# PE32+ data-directory entries: imports at index 1, delay imports at index 13.
if ([BitConverter]::ToUInt32($bytes, $optional + 112 + 13 * 8) -ne 0) {
    throw 'Unexpected delay-loaded dependencies in OCR executable.'
}
$descriptor = Get-FileOffset ([BitConverter]::ToUInt32($bytes, $optional + 120))
$imports = @()
while ([BitConverter]::ToUInt32($bytes, $descriptor + 12) -ne 0) {
    $start = Get-FileOffset ([BitConverter]::ToUInt32($bytes, $descriptor + 12))
    $end = $start
    while ($bytes[$end] -ne 0) { $end++ }
    $name = [Text.Encoding]::ASCII.GetString($bytes, $start, $end - $start)
    if ($name -ine 'KERNEL32.dll') { throw "Unexpected OCR dependency: $name. Review its license before bundling." }
    $imports += $name
    $descriptor += 20
}
if ($imports.Count -ne 1) { throw 'Expected only KERNEL32.dll in the OCR import table.' }
Write-Host "OCR imports verified: $($imports -join ', ')"
