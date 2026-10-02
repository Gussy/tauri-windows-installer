$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. "$PSScriptRoot/helpers/pe-resources.ps1"
$work = Join-Path ([IO.Path]::GetTempPath()) ('twi-pe-reader-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    # A synthetic, non-executable PE exercises numeric and named resource trees.
    # No fixture is loaded into the process or launched.
    [byte[]]$image = [byte[]]::new(0x1200)
    function Put16([int]$offset, [uint16]$value) { [BitConverter]::GetBytes($value).CopyTo($image, $offset) }
    function Put32([int]$offset, [uint32]$value) { [BitConverter]::GetBytes($value).CopyTo($image, $offset) }
    function Directory([int]$relative, [uint16]$named, [uint16]$ids) { Put16 (0x200 + $relative + 12) $named; Put16 (0x200 + $relative + 14) $ids }
    function Entry([int]$relative, [uint32]$key, [uint32]$target) { Put32 (0x200 + $relative) $key; Put32 (0x200 + $relative + 4) $target }
    function Name([int]$relative, [string]$name) { Put16 (0x200 + $relative) $name.Length; [Text.Encoding]::Unicode.GetBytes($name).CopyTo($image, 0x202 + $relative) }
    function Payload([int]$leaf, [int]$relative, [byte[]]$bytes) { Put32 (0x200 + $leaf) (0x1000 + $relative); Put32 (0x204 + $leaf) $bytes.Length; $bytes.CopyTo($image, 0x200 + $relative) }
    Put16 0 0x5a4d; Put32 0x3c 0x80
    Put32 0x80 0x4550; Put16 0x84 0x8664; Put16 0x86 1; Put16 0x94 240
    Put16 0x98 0x20b; Put32 0x118 0x1000
    Put32 0x194 0x1000; Put32 0x198 0x1000; Put32 0x19c 0x200
    Directory 0 0 2; Entry 0x10 10 0x80000040L; Entry 0x18 24 0x80000060L
    Directory 0x40 2 0; Entry 0x50 0x80000180L 0x80000080L; Entry 0x58 0x80000198L 0x800000a0L
    Directory 0x60 0 1; Entry 0x70 1 0x800000c0L
    Directory 0x80 0 1; Entry 0x90 1033 0xe0
    Directory 0xa0 0 1; Entry 0xb0 1033 0xf0
    Directory 0xc0 0 1; Entry 0xd0 1033 0x100
    Name 0x180 'TWI_BUNDLE'; Name 0x198 'TWI_MANIFEST'
    [byte[]]$bundle = 1, 2, 3, 4, 255
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $hash = ([BitConverter]::ToString($sha.ComputeHash($bundle))).Replace('-', '').ToLowerInvariant() } finally { $sha.Dispose() }
    $manifest = @{ format_version = 1; manifest = @{ version = '1.2.3'; unpacked_size = 5; bundle_sha256 = $hash } }
    [byte[]]$manifestBytes = [Text.Encoding]::UTF8.GetBytes("TWI-MANIFEST`n" + ($manifest | ConvertTo-Json -Depth 4 -Compress))
    [byte[]]$xml = [Text.Encoding]::UTF8.GetBytes('<assembly><trustInfo><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false" /></requestedPrivileges></security></trustInfo></assembly>')
    Payload 0xe0 0x300 $bundle; Payload 0xf0 0x340 $manifestBytes; Payload 0x100 0x700 $xml
    $path = Join-Path $work 'resources.exe'
    [IO.File]::WriteAllBytes($path, $image)
    $decoded = Assert-PackageResources $path
    if ($decoded.version -ne '1.2.3') { throw 'Named manifest resource did not decode' }
    [byte[]]$xmlWithBom = [byte[]](0xef, 0xbb, 0xbf) + $xml
    Payload 0x100 0x700 $xmlWithBom
    [IO.File]::WriteAllBytes($path, $image)
    $null = Assert-PackageResources $path
    $resource = Get-PeResource -Path $path -Name 'TWI_BUNDLE'
    if ($resource.FileOffset -ne 0x500 -or $resource.Size -ne 5) { throw 'Resource offset/size did not match section mapping' }
    $image[$resource.FileOffset] = $image[$resource.FileOffset] -bxor 1
    [IO.File]::WriteAllBytes($path, $image)
    $rejected = $false
    try { $null = Assert-PackageResources $path } catch { $rejected = $_.Exception.Message.Contains('digest') }
    if (-not $rejected) { throw 'A payload mutation bypassed the package digest check' }
    Put32 0x2e4 0xffffffffL
    [IO.File]::WriteAllBytes($path, $image)
    $rejected = $false
    try { $null = Get-PeResource -Path $path -Name 'TWI_BUNDLE' } catch { $rejected = $true }
    if (-not $rejected) { throw 'Truncated payload was accepted' }
    Write-Host 'PE resource reader regression checks passed.'
} finally { Remove-Item -LiteralPath $work -Recurse -Force }
