# Read PE resources without executing or loading the inspected binary.
function Get-PeResource {
    param([Parameter(Mandatory)][string]$Path, [uint32]$Type = 10, [Parameter(Mandatory)][string]$Name)
    [byte[]]$data = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    function Read-U16([int]$offset) { if ($offset -lt 0 -or $offset + 2 -gt $data.Length) { throw 'Truncated PE' }; [BitConverter]::ToUInt16($data, $offset) }
    function Read-U32([int]$offset) { if ($offset -lt 0 -or $offset + 4 -gt $data.Length) { throw 'Truncated PE' }; [BitConverter]::ToUInt32($data, $offset) }
    if ((Read-U16 0) -ne 0x5a4d) { throw 'Not a PE executable' }
    $pe = [int](Read-U32 0x3c)
    if ((Read-U32 $pe) -ne 0x4550 -or (Read-U16 ($pe + 4)) -ne 0x8664) { throw 'Expected AMD64 PE' }
    $sectionCount = Read-U16 ($pe + 6)
    $optional = $pe + 24
    if ((Read-U16 $optional) -ne 0x20b) { throw 'Expected PE32+' }
    $sections = $optional + (Read-U16 ($pe + 20))
    function Convert-Rva([uint32]$rva) {
        for ($i = 0; $i -lt $sectionCount; $i++) {
            $entry = $sections + 40 * $i
            $address = Read-U32 ($entry + 12); $rawSize = Read-U32 ($entry + 16)
            if ($rva -ge $address -and [uint64]$rva -lt ([uint64]$address + $rawSize)) {
                return [int]((Read-U32 ($entry + 20)) + $rva - $address)
            }
        }
        throw 'PE resource RVA outside file sections'
    }
    $resourceRva = Read-U32 ($optional + 112 + 16)
    if ($resourceRva -eq 0) { throw 'PE has no resource directory' }
    $resourceBase = Convert-Rva $resourceRva
    function Find-Entry([int]$table, [string]$wanted) {
        $count = (Read-U16 ($table + 12)) + (Read-U16 ($table + 14))
        for ($j = 0; $j -lt $count; $j++) {
            $entry = $table + 16 + 8 * $j
            $key = Read-U32 $entry
            if ($key -band 0x80000000L) {
                $textOffset = $resourceBase + ($key -band 0x7fffffff)
                $length = Read-U16 $textOffset
                if ($textOffset + 2 + $length * 2 -gt $data.Length) { throw 'Invalid PE resource name' }
                $text = [Text.Encoding]::Unicode.GetString($data, $textOffset + 2, $length * 2)
            } else { $text = [string]$key }
            if ($text -ceq $wanted) { return (Read-U32 ($entry + 4)) }
        }
        throw "Missing PE resource: $wanted"
    }
    $typeEntry = Find-Entry $resourceBase ([string]$Type)
    if (-not ($typeEntry -band 0x80000000L)) { throw 'Invalid PE type entry' }
    $nameEntry = Find-Entry ($resourceBase + ($typeEntry -band 0x7fffffff)) $Name
    if (-not ($nameEntry -band 0x80000000L)) { throw 'Invalid PE name entry' }
    $languageTable = $resourceBase + ($nameEntry -band 0x7fffffff)
    if ((Read-U16 ($languageTable + 12)) + (Read-U16 ($languageTable + 14)) -lt 1) { throw 'Missing PE resource language' }
    $leaf = Read-U32 ($languageTable + 20)
    if ($leaf -band 0x80000000L) { throw 'Invalid PE resource leaf' }
    $leafOffset = $resourceBase + $leaf
    $offset = Convert-Rva (Read-U32 $leafOffset); $size = Read-U32 ($leafOffset + 4)
    if ($size -eq 0 -or [uint64]$offset + $size -gt $data.Length) { throw 'Empty or truncated resource payload' }
    [byte[]]$bytes = $data[$offset..($offset + $size - 1)]
    [pscustomobject]@{ FileOffset = $offset; Size = $size; Bytes = $bytes }
}

function Assert-PackageResources {
    param([string]$Path, [int]$FormatVersion = 1)
    $xmlText = [Text.Encoding]::UTF8.GetString((Get-PeResource -Path $Path -Type 24 -Name '1').Bytes)
    # UTF-8 decoding retains the compiler's optional BOM as U+FEFF.
    $xmlText = $xmlText.TrimStart([char]0xfeff)
    [xml]$xml = $xmlText
    $level = $xml.SelectSingleNode("//*[local-name()='requestedExecutionLevel']")
    if (-not $level -or $level.level -ne 'asInvoker' -or $level.uiAccess -ne 'false') { throw 'Package must retain the asInvoker Windows manifest' }
    $text = [Text.Encoding]::UTF8.GetString((Get-PeResource -Path $Path -Name 'TWI_MANIFEST').Bytes)
    $prefix = "TWI-MANIFEST`n"
    if (-not $text.StartsWith($prefix)) { throw 'Missing versioned manifest prefix' }
    $envelope = $text.Substring($prefix.Length) | ConvertFrom-Json
    if ($envelope.format_version -ne $FormatVersion) { throw 'Unexpected manifest format version' }
    $bundle = (Get-PeResource -Path $Path -Name 'TWI_BUNDLE').Bytes
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $hash = ([BitConverter]::ToString($sha.ComputeHash($bundle))).Replace('-', '').ToLowerInvariant() } finally { $sha.Dispose() }
    if ($hash -cne $envelope.manifest.bundle_sha256) { throw 'Package payload digest does not match manifest' }
    if ($envelope.manifest.unpacked_size -le 0) { throw 'Missing unpacked size' }
    return $envelope.manifest
}
