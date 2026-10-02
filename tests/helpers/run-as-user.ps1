param([Parameter(Mandatory)][string]$User, [Parameter(Mandatory)][string]$EncodedCommand, [int]$TimeoutSeconds = 180)
$ErrorActionPreference = 'Stop'
$id = [Guid]::NewGuid().ToString('N')
$dir = Join-Path 'C:\temp\twi-e2e' $id
$name = 'TWI-E2E-' + $id
$code = 1
New-Item -ItemType Directory -Path $dir -Force | Out-Null
& icacls.exe $dir /grant ($User + ':(OI)(CI)M') | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot grant test account access' }
$script = Join-Path $dir 'run.ps1'
$out = Join-Path $dir 'stdout.txt'; $err = Join-Path $dir 'stderr.txt'; $result = Join-Path $dir 'exit.txt'
# EncodedCommand is base64, so it cannot inject script syntax here.
$body = '& powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + $EncodedCommand + ' 1> "' + $out + '" 2> "' + $err + '"; [IO.File]::WriteAllText("' + $result + '", [string]$LASTEXITCODE)'
[IO.File]::WriteAllText($script, $body)
try {
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ('-NoProfile -NonInteractive -File "' + $script + '"')
    $principal = New-ScheduledTaskPrincipal -UserId $User -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName $name
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while (-not (Test-Path -LiteralPath $result)) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Command timed out: verify the standard test account is logged in.' }
        Start-Sleep -Milliseconds 200
    }
    if (Test-Path -LiteralPath $out) { [Console]::Out.Write((Get-Content -Raw -LiteralPath $out)) }
    if (Test-Path -LiteralPath $err) { [Console]::Error.Write((Get-Content -Raw -LiteralPath $err)) }
    $code = [int](Get-Content -Raw -LiteralPath $result)
} finally {
    Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
}
exit $code
