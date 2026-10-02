# Optional signing is configured by the maintainer; argument boundaries survive.
param([string[]]$Files = @('target\release\setup.exe', 'target\release\bundler.exe'))
$ErrorActionPreference = 'Stop'
if (-not $env:TWI_RELEASE_SIGN_PROGRAM) { throw 'Missing TWI_RELEASE_SIGN_PROGRAM' }
[string[]]$arguments = $env:TWI_RELEASE_SIGN_ARGS | ConvertFrom-Json
foreach ($file in $Files) {
    $resolved = (Resolve-Path -LiteralPath $file).Path
    $hasPlaceholder = $false
    $expanded = foreach ($argument in $arguments) {
        if ($argument.Contains('%1')) { $hasPlaceholder = $true }
        $argument.Replace('%1', $resolved)
    }
    if (-not $hasPlaceholder) { $expanded = @($expanded) + @($resolved) }
    & $env:TWI_RELEASE_SIGN_PROGRAM @expanded | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Release signer failed' }
    $signature = Get-AuthenticodeSignature -LiteralPath $resolved
    if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw 'Signer did not produce a valid timestamped signature' }
}
