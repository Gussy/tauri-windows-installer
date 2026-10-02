param(
    [string]$SetupStub = 'target\release\setup.exe',
    [string]$Bundler = 'target\release\bundler.exe',
    [Parameter(Mandatory)][string]$ExpectedVersion,
    [string]$Commit = '',
    [switch]$RequireSignature
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. "$PSScriptRoot\helpers\pe-resources.ps1"
$infoText = & $Bundler --setup-exe $SetupStub --stub-info
if ($LASTEXITCODE -ne 0) { throw 'Stub inspection failed' }
$info = $infoText | ConvertFrom-Json
if ($info.package_version -ne $ExpectedVersion -or $info.format_version -ne 1 -or $info.architecture -ne 'x86_64' -or -not $info.abi_compatible -or -not $info.manifest_as_invoker) { throw 'Bundler version / schema / ABI / manifest mismatch' }
$hash = (Get-FileHash -LiteralPath $SetupStub -Algorithm SHA256).Hash.ToLowerInvariant()
if ($info.stub_sha256 -cne $hash) { throw 'Bundler embeds a different setup stub' }
# Verify the actual embedded stub too, not just a supplied override.
$embeddedText = & $Bundler --stub-info
if ($LASTEXITCODE -ne 0) { throw 'Embedded stub inspection failed' }
$embedded = $embeddedText | ConvertFrom-Json
if ($embedded.stub_sha256 -cne $hash) { throw 'Stale setup stub embedded in bundler' }

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $vsPath) { throw 'Visual Studio tools not found' }
$dumpbin = Get-ChildItem "$vsPath\VC\Tools\MSVC" -Filter dumpbin.exe -Recurse -File | Where-Object { $_.FullName -match '\\Hostx64\\x64\\dumpbin\.exe$' } | Select-Object -First 1
if (-not $dumpbin) { throw 'dumpbin not found' }
$files = @()
foreach ($binary in @($SetupStub, $Bundler)) {
    $dependencies = & $dumpbin.FullName /dependents $binary
    if ($LASTEXITCODE -ne 0) { throw "Cannot inspect runtime dependencies: $binary" }
    if ($dependencies | Select-String '\b(?:VCRUNTIME|MSVCP)[^\s]*\.dll\b') { throw "$binary needs an external MSVC runtime" }
    $signature = Get-AuthenticodeSignature -LiteralPath $binary
    if ($RequireSignature -and ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate)) { throw "$binary needs a valid timestamped Authenticode signature" }
    $binaryHash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    $filename = Split-Path -Leaf $binary
    "$binaryHash  $filename" | Set-Content -LiteralPath ($binary + '.sha256') -Encoding ascii
    $files += @{ name = $filename; sha256 = $binaryHash; signature_status = [string]$signature.Status }
}
@{ commit = $Commit; package_version = $ExpectedVersion; format_version = $info.format_version; stub_sha256 = $hash; files = $files } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path (Split-Path $Bundler) 'provenance.json') -Encoding utf8
Write-Host 'Verified release versions, stub hash/ABI/manifest, signatures policy, and static MSVC runtime.'
