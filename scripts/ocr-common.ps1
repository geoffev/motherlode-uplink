# Avoid module autoloading: CI may launch Windows PowerShell from PowerShell 7,
# where the inherited module search path can make Get-FileHash unavailable.
function Get-OcrSha256([string]$Path) {
    $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    $stream = [IO.File]::OpenRead($resolved)
    try {
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try {
            return [BitConverter]::ToString($sha256.ComputeHash($stream)).Replace('-', '').ToLowerInvariant()
        } finally { $sha256.Dispose() }
    } finally { $stream.Dispose() }
}
