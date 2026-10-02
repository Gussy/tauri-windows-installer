param([string]$Worker = "$PSScriptRoot\..\uninstaller\src\cleanup.ps1")
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $Worker), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
# Load only functions from the reviewed local source, never its process/registry
# entry point. This permits filesystem guard checks on macOS and Linux too.
foreach ($node in $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $true)) {
    Invoke-Expression $node.Extent.Text
}
SafeName 'com.example.fixture'
foreach ($bad in @('', '..', '../outside', 'NUL.exe', 'bad\child', 'bad:stream', 'trailing.')) {
    $rejected = $false
    try { SafeName $bad } catch { $rejected = $true }
    if (-not $rejected) { throw "Cleanup accepted unsafe filename: $bad" }
}
$temporary = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
try {
    $managed = Join-Path $temporary 'managed'; $outside = Join-Path $temporary 'outside'
    New-Item -ItemType Directory -Path $managed, $outside | Out-Null
    $sentinel = Join-Path $outside 'preserve.txt'
    [IO.File]::WriteAllText($sentinel, 'preserve')
    $statePath = Join-Path $temporary 'state.json'
    [IO.File]::WriteAllText($statePath, '{"id":"valid-state"}', [Text.UTF8Encoding]::new($false))
    if ((ReadState $statePath).id -ne 'valid-state') { throw 'Valid recovery state did not decode' }
    [IO.File]::WriteAllBytes($statePath, [byte[]]::new(65537))
    $rejected = $false
    try { $null = ReadState $statePath } catch { $rejected = $_.Exception.Message.Contains('too large') }
    if (-not $rejected) { throw 'Oversized recovery state bypassed the bounded read' }
    [IO.File]::WriteAllBytes($statePath, [byte[]]@(0x7b, 0x22, 0xff, 0x22, 0x3a, 0x31, 0x7d))
    $rejected = $false
    try { $null = ReadState $statePath } catch { $rejected = $_.Exception.GetBaseException() -is [Text.DecoderFallbackException] }
    if (-not $rejected) { throw 'Recovery state did not reject invalid UTF-8 strictly' }
    $link = Join-Path $managed 'outside-link'
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        New-Item -ItemType Junction -Path $link -Target $outside | Out-Null
    } else {
        New-Item -ItemType SymbolicLink -Path $link -Target $outside | Out-Null
    }
    $rejected = $false
    try { NoReparse $link } catch { $rejected = $true }
    if (-not $rejected) { throw 'Cleanup accepted a reparse recovery path' }
    $readonly = Join-Path $managed 'readonly.txt'
    [IO.File]::WriteAllText($readonly, 'remove')
    [IO.File]::SetAttributes($readonly, [IO.FileAttributes]::ReadOnly)
    # More nesting than PowerShell's recursion limit, with a >260-character asset
    # path. Device paths on Windows must also work under Windows PowerShell 5.1.
    $deep = $managed
    for ($depth = 0; $depth -lt 256; $depth++) { $deep = [IO.Path]::Combine($deep, 'd') }
    [IO.Directory]::CreateDirectory((FilePath $deep)) | Out-Null
    $longAsset = [IO.Path]::Combine($deep, ('a' * 180) + '.bin')
    [IO.File]::WriteAllText((FilePath $longAsset), 'long deep asset')
    if (-not [IO.File]::Exists((FilePath $longAsset))) { throw 'Long asset fixture could not be created' }
    $brokenTarget = Join-Path $temporary 'gone-target'
    [IO.Directory]::CreateDirectory($brokenTarget) | Out-Null
    $brokenLink = Join-Path $managed 'broken-link'
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        New-Item -ItemType Junction -Path $brokenLink -Target $brokenTarget | Out-Null
    } else {
        New-Item -ItemType SymbolicLink -Path $brokenLink -Target $brokenTarget | Out-Null
    }
    [IO.Directory]::Delete($brokenTarget)
    $rejected = $false
    try { NoReparse $brokenLink } catch { $rejected = $true }
    if (-not $rejected) { throw 'Cleanup accepted a broken reparse recovery path' }
    DeleteEntry $managed
    if ((Test-Path $managed) -or -not (Test-Path $sentinel) -or (Get-Content -Raw $sentinel) -ne 'preserve') { throw 'Cleanup followed a link outside directory ownership' }
} finally { Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue }
Write-Host 'Cleanup bounded UTF-8 state, deep/long paths and live/broken link checks passed.'
