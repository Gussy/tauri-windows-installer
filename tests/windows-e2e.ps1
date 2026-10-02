param(
    [string]$SetupStub = 'target\release\setup.exe',
    [string]$Bundler = 'target\release\bundler.exe',
    [string]$Fixture = 'target\release\examples\twi-test-app.exe',
    [switch]$RequireStandardUser,
    [string]$ArtifactDirectory = 'target\e2e-results'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. "$PSScriptRoot\helpers\pe-resources.ps1"
$SetupStub = (Resolve-Path -LiteralPath $SetupStub).Path
$Bundler = (Resolve-Path -LiteralPath $Bundler).Path
$Fixture = (Resolve-Path -LiteralPath $Fixture).Path
New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
$ArtifactDirectory = (Resolve-Path -LiteralPath $ArtifactDirectory).Path
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($identity.IsSystem) { throw 'Run native tests as an actual user, not SYSTEM.' }
if ($RequireStandardUser) {
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Expected a standard user token' }
    if ((Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System').EnableLUA -ne 1) { throw 'UAC must remain enabled' }
    if (-not (Get-MpComputerStatus).RealTimeProtectionEnabled) { throw 'Defender must remain enabled' }
}
$id = 'com.gussy.twi-e2e.' + [Guid]::NewGuid().ToString('N')
$work = Join-Path ([IO.Path]::GetTempPath()) ('TWI Unicode ü tests ' + [Guid]::NewGuid().ToString('N'))
$local = [Environment]::GetFolderPath('LocalApplicationData')
$root = Join-Path (Join-Path $local 'Programs') $id
$dataRoot = Join-Path $local $id
$registry = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\' + $id
$title1 = 'TWI old ' + $id; $title2 = 'TWI new ' + $id
$desktop = [Environment]::GetFolderPath('Desktop')
$shortcut1 = Join-Path $desktop ($title1 + '.lnk'); $shortcut2 = Join-Path $desktop ($title2 + '.lnk')
$main1 = Join-Path $root 'app.exe'; $main2 = Join-Path $root 'bin\app.exe'
$setupFiles = @{}
$passed = [Collections.Generic.List[string]]::new()
function Assert([bool]$condition, [string]$message) { if (-not $condition) { throw $message } }
function Invoke-Checked([string]$program, [string[]]$arguments) {
    & $program @arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$program exited $LASTEXITCODE" }
}
function Run-Setup([string]$path, [bool]$expectSuccess = $true, [bool]$launch = $false) {
    $arguments = @('--silent')
    if (-not $launch) { $arguments += '--no-launch' }
    $process = Start-Process -FilePath $path -ArgumentList $arguments -WorkingDirectory $work -PassThru
    if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Setup timed out' }
    $process.Refresh()
    Assert (($process.ExitCode -eq 0) -eq $expectSuccess) "Unexpected setup exit: $($process.ExitCode)"
}
function Wait-Cleanup {
    $deadline = [DateTime]::UtcNow.AddSeconds(45)
    do {
        $leftovers = @(Get-ChildItem -LiteralPath (Split-Path $root) -Directory | Where-Object { $_.Name.StartsWith($id) })
        if ($leftovers.Count -eq 0 -and -not (Test-Path -LiteralPath $registry) -and -not (Test-Path -LiteralPath ($root + '.twi-uninstall.json'))) { return }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'Original or renamed uninstall directory / registration remains'
}
function Package([string]$version, [string]$application, [string]$title, [bool]$shortcut = $true) {
    $source = Join-Path $work ('source-' + $version + '-' + $shortcut)
    $exe = Join-Path $source $application
    New-Item -ItemType Directory -Path (Split-Path $exe) -Force | Out-Null
    Copy-Item -LiteralPath $Fixture -Destination $exe
    [IO.File]::WriteAllText((Join-Path $source ('sidecar-' + $version + '.txt')), $version)
    $config = Join-Path $source 'tauri.conf.json'
    @{ productName = 'twi-fixture'; version = $version; identifier = $id; plugins = @{ 'tauri-windows-installer' = @{ desktopShortcut = $shortcut; webview2 = @{ bundle = 'evergreen' } } } } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $config -Encoding utf8
    $output = Join-Path $work ('package-' + $version + '-' + $shortcut)
    Invoke-Checked $Bundler @('--setup-exe', $SetupStub, '--tauri-conf', $config, '--app', $source, '--main-exe', $application, '--title', $title, '--output-dir', $output)
    $setup = Join-Path $output 'twi-fixture-setup.exe'
    $manifest = Assert-PackageResources $setup
    Assert ($manifest.version -eq $version) 'Packaged version mismatch'
    return $setup
}
try {
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    $setupFiles.v1 = Package '1.0.0' 'app.exe' $title1
    $setupFiles.v2 = Package '2.0.0' 'bin/app.exe' $title2
    $setupFiles.noShortcut = Package '2.0.1' 'bin/app.exe' $title2 $false
    Run-Setup $setupFiles.v1 $true $true
    Assert (Test-Path -LiteralPath $main1) 'Fresh install missing main exe'
    Assert ((Get-ItemPropertyValue $registry DisplayVersion) -eq '1.0.0') 'Fresh registry version mismatch'
    Assert (Test-Path -LiteralPath $shortcut1) 'Fresh desktop shortcut missing'
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath (Join-Path $root '.fixture-cwd.txt')) -or
        [string]::IsNullOrEmpty([string](Get-Content -Raw -LiteralPath (Join-Path $root '.fixture-cwd.txt')))) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Installer did not launch the fixture' }
        Start-Sleep -Milliseconds 100
    }
    Assert ((Get-Content -Raw (Join-Path $root '.fixture-cwd.txt')) -eq $root) 'Initial app launch inherited the wrong cwd'
    $launched = @(Get-Process -Name app -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $main1 })
    Assert ($launched.Count -eq 1) 'Expected one newly launched fixture'
    $launched[0].Kill(); $null = $launched[0].WaitForExit(5000)
    New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $dataRoot 'user-state.txt'), 'preserve me')
    $passed.Add('fresh install, metadata/registry/shortcut, package schema/manifest/digest')

    $signal = Join-Path $work 'lock-ready.txt'
    $lockProcess = Start-Process -FilePath $main1 -ArgumentList '--test-hold-lock', ('"' + $root + '"'), ('"' + $signal + '"') -PassThru
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path -LiteralPath $signal)) {
            if ($lockProcess.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'Fixture lock was not acquired' }
            Start-Sleep -Milliseconds 100
        }
        Run-Setup $setupFiles.v2 $false
        $busyUninstall = Start-Process -FilePath $main1 -ArgumentList '--uninstall','--silent' -PassThru
        Assert ($busyUninstall.WaitForExit(10000)) 'Busy uninstall did not return'
        $busyUninstall.Refresh()
        Assert ($busyUninstall.ExitCode -ne 0) 'Uninstall bypassed shared operation lock'
        Assert (-not $lockProcess.HasExited) 'Busy setup killed lock owner'
        Assert ((Get-ItemPropertyValue $registry DisplayVersion) -eq '1.0.0') 'Busy operation changed installed version'
    } finally {
        if (-not $lockProcess.HasExited) { $lockProcess.Kill(); $null = $lockProcess.WaitForExit(5000) }
    }
    $passed.Add('setup and uninstall reject overlapping operation without modifying installation')

    # Hold a directory handle without FILE_SHARE_DELETE: directory swap must fail.
    Add-Type @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class TwiDirectoryLock {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
}
'@
    # FILE_LIST_DIRECTORY participates in sharing checks; zero access only
    # queries metadata and does not reliably prevent a directory rename.
    $handle = [TwiDirectoryLock]::CreateFile($root, 1, 3, [IntPtr]::Zero, 3, 0x02000000, [IntPtr]::Zero)
    Assert (-not $handle.IsInvalid) 'Could not acquire deterministic directory lock'
    try {
        $probe = [TwiDirectoryLock]::CreateFile($root, 0x10000, 7, [IntPtr]::Zero, 3, 0x02000000, [IntPtr]::Zero)
        $probeError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        try { Assert ($probe.IsInvalid -and $probeError -eq 32) 'Directory fixture does not deny DELETE access' }
        finally { $probe.Dispose() }
        Run-Setup $setupFiles.v2 $false
    } finally { $handle.Dispose() }
    Assert (Test-Path -LiteralPath $main1) 'Failed upgrade lost old executable'
    Assert ((Get-ItemPropertyValue $registry DisplayVersion) -eq '1.0.0') 'Failed upgrade changed registration'
    Assert ((Get-Content -Raw (Join-Path $root '.twi-meta.json') | ConvertFrom-Json).version -eq '1.0.0') 'Failed upgrade changed metadata'
    $passed.Add('locked-directory failure preserves version one and uninstall metadata')

    $corrupt = Join-Path $work 'corrupt-setup.exe'
    Copy-Item -LiteralPath $setupFiles.v2 -Destination $corrupt
    $resource = Get-PeResource -Path $corrupt -Name 'TWI_BUNDLE'
    [byte[]]$bytes = [IO.File]::ReadAllBytes($corrupt)
    $position = $resource.FileOffset + [int]($resource.Size / 2)
    $bytes[$position] = $bytes[$position] -bxor 1
    [IO.File]::WriteAllBytes($corrupt, $bytes)
    Run-Setup $corrupt $false
    Assert (Test-Path -LiteralPath $main1) 'Corrupt package destroyed existing installation'
    Assert ((Get-ItemPropertyValue $registry DisplayVersion) -eq '1.0.0') 'Corrupt package changed registry'
    $passed.Add('corrupt payload rejected before replacing working install')

    # Recreate a process-loss boundary after the old root was retired and before
    # stage promotion. The next real setup must recover this durable journal.
    $nonce = [Guid]::NewGuid().ToString('N').Substring(0, 24)
    $backup = $root + '.twi-backup-' + $nonce
    $stage = $root + '.twi-stage-' + $nonce
    $snapshotKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\' + $id)
    $snapshot = @()
    try {
        foreach ($name in $snapshotKey.GetValueNames()) {
            $kind = [int]$snapshotKey.GetValueKind($name)
            $value = $snapshotKey.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            [byte[]]$raw = switch ($kind) {
                1 { [Text.Encoding]::Unicode.GetBytes([string]$value + [char]0) }
                2 { [Text.Encoding]::Unicode.GetBytes([string]$value + [char]0) }
                3 { [byte[]]$value }
                4 { [BitConverter]::GetBytes([uint32]$value) }
                7 { [Text.Encoding]::Unicode.GetBytes(($value -join [char]0) + [char]0 + [char]0) }
                11 { [BitConverter]::GetBytes([uint64]$value) }
                default { throw "Unexpected registry fixture kind: $kind" }
            }
            $snapshot += @{ name = $name; kind = $kind; bytes = @($raw | ForEach-Object { [int]$_ }) }
        }
    } finally { $snapshotKey.Dispose() }
    $shortcutBytes = [IO.File]::ReadAllBytes($shortcut1)
    $journal = @{ version = 1; nonce = $nonce; old_exists = $true; phase = 'switch_started'; external = @{ identifier = $id; registry = $snapshot; shortcuts = @(@{ title = $title1; bytes = @($shortcutBytes | ForEach-Object { [int]$_ }) }) } }
    Move-Item -LiteralPath $root -Destination $backup
    New-Item -ItemType Directory -Path $stage | Out-Null
    [IO.File]::WriteAllText((Join-Path $stage 'incomplete.txt'), 'uncommitted stage')
    [IO.File]::WriteAllText(($root + '.twi-transaction.json'), ($journal | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
    Run-Setup $setupFiles.v1 $true $true
    Assert (Test-Path -LiteralPath $main1) 'Interrupted transaction did not restore a working root'
    Assert ((Get-ItemPropertyValue $registry DisplayVersion) -eq '1.0.0') 'Recovery lost registry state'
    Assert (-not (Test-Path -LiteralPath $backup) -and -not (Test-Path -LiteralPath $stage) -and -not (Test-Path -LiteralPath ($root + '.twi-transaction.json'))) 'Recovered transaction residue remains'
    $passed.Add('interrupted switch journal restores old installation before retry')

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath (Join-Path $root '.fixture-cwd.txt')) -or
        [string]::IsNullOrEmpty([string](Get-Content -Raw -LiteralPath (Join-Path $root '.fixture-cwd.txt')))) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Recovered install did not launch the fixture' }
        Start-Sleep -Milliseconds 100
    }
    Assert ((Get-Content -Raw (Join-Path $root '.fixture-cwd.txt')) -eq $root) 'Installer-launched app inherited the wrong cwd'
    $launched = @(Get-Process -Name app -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $main1 })
    Assert ($launched.Count -eq 1) 'Expected one live installer-launched fixture before upgrade'
    $oldProcess = $launched[0]
    Assert (-not $oldProcess.HasExited) 'Installer-launched fixture exited before replacement'
    Run-Setup $setupFiles.v2
    Assert ($oldProcess.WaitForExit(5000)) 'Original process survived upgrade'
    Assert (Test-Path -LiteralPath $main2) 'Nested new executable missing'
    Assert (-not (Test-Path -LiteralPath $main1)) 'Removed executable remains'
    Assert (-not (Test-Path -LiteralPath (Join-Path $root 'sidecar-1.0.0.txt'))) 'Obsolete sidecar remains'
    Assert ((Get-Content -Raw (Join-Path $root 'sidecar-2.0.0.txt')) -eq '2.0.0') 'New sidecar absent'
    Assert ((Get-Content -Raw (Join-Path $dataRoot 'user-state.txt')) -eq 'preserve me') 'Upgrade removed user data'
    Assert ((Get-ItemPropertyValue $registry DisplayVersion) -eq '2.0.0') 'Upgrade registry version mismatch'
    Assert (-not (Test-Path -LiteralPath $shortcut1)) 'Old title shortcut remains'
    Assert (Test-Path -LiteralPath $shortcut2) 'New title shortcut missing'
    $passed.Add('installer launch uses install cwd; upgrade kills its actual PID, updates nested exe/sidecars/shortcut, preserves data')

    Remove-Item -LiteralPath $main2 -Force
    Run-Setup $setupFiles.v2
    Assert (Test-Path -LiteralPath $main2) 'Repair did not restore missing executable'
    Run-Setup $setupFiles.noShortcut
    Assert (-not (Test-Path -LiteralPath $shortcut2)) 'Disabling shortcut left owned link'
    $passed.Add('repair and explicit shortcut disable')

    # Application-created caches can exceed MAX_PATH and contain junctions.
    # Real deferred cleanup must unlink these entries without following targets.
    $deepCache = $root
    for ($depth = 0; $depth -lt 256; $depth++) { $deepCache = [IO.Path]::Combine($deepCache, 'd') }
    $deepCache = '\\?\' + $deepCache
    [IO.Directory]::CreateDirectory($deepCache) | Out-Null
    [IO.File]::WriteAllText([IO.Path]::Combine($deepCache, ('a' * 180) + '.bin'), 'deep application cache')
    New-Item -ItemType Junction -Path (Join-Path $root 'external-data-link') -Target $dataRoot | Out-Null
    $goneTarget = Join-Path $work 'gone-target'
    [IO.Directory]::CreateDirectory($goneTarget) | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $root 'broken-cache-link') -Target $goneTarget | Out-Null
    [IO.Directory]::Delete($goneTarget)

    # Launch the nested executable from its install cwd to exercise root discovery
    # and ensure the cleanup helper waits for the uninstaller to exit.
    $uninstall = Start-Process -FilePath $main2 -ArgumentList '--uninstall','--silent' -WorkingDirectory $root -PassThru
    if (-not $uninstall.WaitForExit(30000)) { $uninstall.Kill(); throw 'Uninstaller timed out' }
    $uninstall.Refresh()
    Assert ($uninstall.ExitCode -eq 0) "Uninstall exited $($uninstall.ExitCode)"
    Wait-Cleanup
    Assert ((Get-Content -Raw (Join-Path $dataRoot 'user-state.txt')) -eq 'preserve me') 'Uninstall removed user data'
    $passed.Add('nested uninstall removes self, deep long assets and broken junctions; preserves external linked data')

    # An exclusive sidecar lock must leave an independent, registered cleanup
    # command. Wait for its retry budget to expire before releasing the lock.
    Run-Setup $setupFiles.noShortcut
    $sidecar = Join-Path $root 'held-sidecar.dll'
    [IO.File]::WriteAllText($sidecar, 'locked fixture sidecar')
    $held = [IO.FileStream]::new($sidecar, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    try {
        $failedUninstall = Start-Process -FilePath $main2 -ArgumentList '--uninstall','--silent' -WorkingDirectory $root -PassThru
        Assert ($failedUninstall.WaitForExit(30000)) 'Uninstall parent did not exit'
        $failedUninstall.Refresh()
        Assert ($failedUninstall.ExitCode -eq 0) 'Parent did not register deferred cleanup'
        $marker = $root + '.twi-uninstall.json'
        Assert (Test-Path -LiteralPath $marker) 'Failed cleanup lost its recovery marker'
        $state = Get-Content -Raw -LiteralPath $marker | ConvertFrom-Json
        $cleanupLog = Join-Path $state.worker 'cleanup.log'
        $deadline = [DateTime]::UtcNow.AddSeconds(45)
        do {
            if ((Test-Path -LiteralPath $cleanupLog) -and ([string](Get-Content -Raw -LiteralPath $cleanupLog)).Contains('Cleanup failed:')) { break }
            if ([DateTime]::UtcNow -ge $deadline) { throw 'Locked-file worker did not retain a completed failed attempt' }
            Start-Sleep -Milliseconds 200
        } while ($true)
        Assert (Test-Path -LiteralPath $registry) 'Failed cleanup removed retry registration'
        $retry = Get-ItemPropertyValue -LiteralPath $registry -Name UninstallString
        Assert ($retry.Contains('cleanup.ps1') -and $retry.Contains('-StatePath')) 'Retry depends on the partially removed application'
        Copy-Item -LiteralPath $cleanupLog -Destination (Join-Path $ArtifactDirectory 'failed-uninstall.log')
        Run-Setup $setupFiles.noShortcut $false
        Assert (Test-Path -LiteralPath $marker) 'Setup bypassed pending uninstall state'
    } finally { $held.Dispose() }
    $parts = [regex]::Matches($retry, '"([^"]+)"')
    Assert ($parts.Count -eq 3) 'Unexpected standalone retry command'
    $retryProcess = Start-Process -FilePath $parts[0].Groups[1].Value -ArgumentList '-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File', ('"' + $parts[1].Groups[1].Value + '"'), '-StatePath', ('"' + $parts[2].Groups[1].Value + '"') -PassThru
    Assert ($retryProcess.WaitForExit(45000)) 'Standalone retry timed out'
    $retryProcess.Refresh()
    Assert ($retryProcess.ExitCode -eq 0) 'Standalone retry failed after lock release'
    Wait-Cleanup
    $passed.Add('locked sidecar retains independent retry entry; registered worker completes after release')
    $passed | Set-Content -LiteralPath (Join-Path $ArtifactDirectory 'passed.txt')
    Write-Host "Native Windows durability checks passed: $($passed.Count)"
} finally {
    if (Test-Path -LiteralPath (Join-Path $dataRoot 'installer.log')) { Copy-Item -LiteralPath (Join-Path $dataRoot 'installer.log') -Destination (Join-Path $ArtifactDirectory 'installer.log') -Force }
    Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Filter 'twi-*.log' -File | Where-Object { $_.LastWriteTimeUtc -ge [DateTime]::UtcNow.AddHours(-1) } | Copy-Item -Destination $ArtifactDirectory -Force
    $workerParent = Join-Path (Split-Path $root) '.twi-uninstall-workers'
    if (Test-Path -LiteralPath $workerParent) {
        Get-ChildItem -LiteralPath $workerParent -Directory | Where-Object { $_.Name.StartsWith($id + '-') } | ForEach-Object {
            if (Test-Path -LiteralPath (Join-Path $_.FullName 'cleanup.log')) {
                Copy-Item -LiteralPath (Join-Path $_.FullName 'cleanup.log') -Destination (Join-Path $ArtifactDirectory ($_.Name + '-cleanup.log')) -Force
            }
        }
    }
    $passed | Set-Content -LiteralPath (Join-Path $ArtifactDirectory 'passed.txt')
    # The identifier is unique and this suite created these paths. Preserve failed
    # installation trees for diagnosis; never sweep arbitrary Programs directories.
    Remove-Item -LiteralPath $shortcut1, $shortcut2 -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
