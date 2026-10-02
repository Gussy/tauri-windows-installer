param([Parameter(Mandatory = $true)][string] $StatePath)
# A retry command independent of the application, its DLLs, and its working directory.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$lockStream = $null
$logPath = Join-Path $PSScriptRoot 'cleanup.log'

function FullPath([string] $path) {
    if ($path.StartsWith('\\?\UNC\')) { $path = '\\' + $path.Substring(8) }
    elseif ($path.StartsWith('\\?\')) { $path = $path.Substring(4) }
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        $path = $path.Replace('/', '\')
        # State paths are already absolute and owned. Avoid GetFullPath's legacy
        # MAX_PATH restriction, and reject dot components rather than resolving them.
        if (($path -notmatch '^[A-Za-z]:\\' -and -not $path.StartsWith('\\')) -or $path -match '(^|\\)\.{1,2}(\\|$)') { throw 'Recovery path must be absolute without dot components' }
        return $path.TrimEnd('\')
    }
    return [IO.Path]::GetFullPath($path).TrimEnd('\')
}
function SamePath([string] $left, [string] $right) {
    return [string]::Equals((FullPath $left), (FullPath $right), [StringComparison]::OrdinalIgnoreCase)
}
function FilePath([string] $path) {
    # Windows PowerShell's .NET APIs need device paths for long installed assets.
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT -and -not $path.StartsWith('\\?\')) {
        if ($path -notmatch '^[A-Za-z]:[\\/]' -and -not $path.StartsWith('\\')) { $path = [IO.Path]::GetFullPath($path) }
        $path = FullPath $path
        if ($path.StartsWith('\\')) { return '\\?\UNC\' + $path.Substring(2) }
        return '\\?\' + $path
    }
    return $path
}
function NoReparse([string] $path) {
    try { $attributes = [IO.File]::GetAttributes((FilePath $path)) }
    catch [IO.FileNotFoundException] { return }
    catch [IO.DirectoryNotFoundException] { return }
    if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Recovery path is a reparse point: $path" }
}
function SafeName([string] $name) {
    if ([string]::IsNullOrEmpty($name) -or $name.Length -gt 200 -or $name -match '[<>:"/\\|?*\x00-\x1f]' -or $name -match '[. ]$' -or $name -in @('.', '..')) { throw 'Invalid recovery filename' }
    if ($name.Split('.')[0] -match '^(CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9¹²³]|LPT[1-9¹²³])$') { throw 'Reserved recovery filename' }
}
function DeleteEntry([string] $path) {
    # Post-order traversal avoids call-stack exhaustion for application-created
    # deep directories. Reparse entries are unlinked without traversing targets.
    $pending = [Collections.Generic.Stack[object]]::new()
    $pending.Push(@{ Path = (FilePath $path); Visited = $false })
    while ($pending.Count -gt 0) {
        $entry = $pending.Pop()
        $attributes = [IO.File]::GetAttributes($entry.Path)
        $directory = ($attributes -band [IO.FileAttributes]::Directory) -ne 0
        $reparse = ($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
        if ($directory -and -not $reparse -and -not $entry.Visited) {
            $pending.Push(@{ Path = $entry.Path; Visited = $true })
            foreach ($child in [IO.Directory]::EnumerateFileSystemEntries($entry.Path)) { $pending.Push(@{ Path = $child; Visited = $false }) }
        } elseif ($directory) { [IO.Directory]::Delete($entry.Path, $false) }
        else {
            if (-not $reparse -and ($attributes -band [IO.FileAttributes]::ReadOnly) -ne 0) { [IO.File]::SetAttributes($entry.Path, ($attributes -band (-bnot [IO.FileAttributes]::ReadOnly))) }
            [IO.File]::Delete($entry.Path)
        }
    }
}
function ReadState([string] $path) {
    NoReparse $path
    $stream = [IO.FileStream]::new((FilePath $path), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        $buffer = [byte[]]::new(65537)
        $length = 0
        while ($length -lt $buffer.Length) {
            $read = $stream.Read($buffer, $length, $buffer.Length - $length)
            if ($read -eq 0) { break }
            $length += $read
        }
        if ($length -gt 65536) { throw 'Recovery state is too large' }
        return ([Text.UTF8Encoding]::new($false, $true).GetString($buffer, 0, $length) | ConvertFrom-Json)
    } finally { $stream.Dispose() }
}
function RegistryOwned($state) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($script:registryPath)
    if ($null -eq $key) { return $false }
    try {
        $location = [string]$key.GetValue('InstallLocation', '')
        return ([string]$key.GetValue('UninstallString', '') -ceq $state.uninstall_command) -and ((SamePath $location $state.root) -or (SamePath $location $state.retired))
    } finally { $key.Dispose() }
}

try {
    $markerExists = [IO.File]::Exists((FilePath $StatePath))
    $readPath = $StatePath
    if (-not $markerExists) { $readPath = Join-Path $PSScriptRoot 'state.json' }
    $state = ReadState $readPath
    $programs = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs'
    $id = [string]$state.metadata.app_id
    SafeName $id
    if ($id.Length -gt 128 -or $id -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -or $state.nonce -notmatch '^[0-9a-f]{32}$' -or $state.format_version -ne 1) { throw 'Invalid recovery identity or format' }
    SafeName ([string]$state.metadata.app_title)
    $exeParts = ([string]$state.metadata.app_exe).Split([char[]]'\/')
    foreach ($part in $exeParts) { SafeName $part }
    if ($state.metadata.app_exe -notmatch '(?i)\.exe$') { throw 'Invalid recovery executable' }
    $root = Join-Path $programs $id
    $retired = Join-Path $programs "$id.twi-uninstall-$($state.nonce)"
    $workerParent = Join-Path $programs '.twi-uninstall-workers'
    $worker = Join-Path $workerParent "$id-$($state.nonce)"
    $marker = Join-Path $programs "$id.twi-uninstall.json"
    $lockParent = Join-Path $programs '.twi-locks'
    $rootFs = FilePath $root
    $retiredFs = FilePath $retired
    if (-not (SamePath $state.programs $programs) -or -not (SamePath $state.root $root) -or -not (SamePath $state.retired $retired) -or -not (SamePath $state.worker $worker) -or -not (SamePath $PSScriptRoot $worker) -or -not (SamePath $StatePath $marker) -or -not (SamePath ([IO.Path]::GetDirectoryName($state.lock)) $lockParent) -or [IO.Path]::GetFileName($state.lock) -notmatch '^[0-9a-f]{64}\.lock$') { throw 'Recovery paths are outside their owned directories' }
    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $command = '"{0}" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{1}" -StatePath "{2}"' -f $ps, (Join-Path $worker 'cleanup.ps1'), $marker
    if ($state.uninstall_command -cne $command) { throw 'Invalid recovery command' }
    foreach ($path in @($programs, $workerParent, $worker, $root, $retired, $lockParent, $state.lock)) { NoReparse $path }
    $script:registryPath = "Software\Microsoft\Windows\CurrentVersion\Uninstall\$id"
    if (-not (RegistryOwned $state)) { throw 'Uninstall registration no longer belongs to this cleanup worker' }
    [IO.File]::WriteAllText((FilePath (Join-Path $worker 'ready')), "$($state.nonce):$($state.parent_pid)", [Text.UTF8Encoding]::new($false))

    # PID reuse cannot cause us to wait on or terminate an unrelated application.
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ($true) {
        $parent = Get-Process -Id $state.parent_pid -ErrorAction SilentlyContinue
        if ($null -eq $parent) { break }
        $start = ([DateTimeOffset]$parent.StartTime.ToUniversalTime()).ToUnixTimeSeconds()
        if ($start -ne $state.parent_start) { break }
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Uninstall parent did not exit' }
        Start-Sleep -Milliseconds 100
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ($null -eq $lockStream) {
        $candidate = $null
        try {
            $candidate = [IO.FileStream]::new($state.lock, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::ReadWrite)
            $candidate.Lock(0, [long]::MaxValue)
            $lockStream = $candidate
        } catch {
            if ($null -ne $candidate) { $candidate.Dispose() }
            if ([DateTime]::UtcNow -ge $deadline) { throw 'Another installation or uninstall owns the operation lock' }
            Start-Sleep -Milliseconds 100
        }
    }
    # Re-read under the lock, preventing a queued older worker from removing newer state.
    if ([IO.File]::Exists((FilePath $marker))) {
        $current = ReadState $marker
        if ($current.nonce -cne $state.nonce) { throw 'A newer uninstall owns recovery state' }
    } elseif ([IO.Directory]::Exists($rootFs) -or [IO.Directory]::Exists($retiredFs)) {
        throw 'Recovery marker is missing while installation files remain'
    }
    if (-not (RegistryOwned $state)) { throw 'Uninstall registration no longer belongs to this cleanup worker' }
    if ([IO.Directory]::Exists($rootFs)) {
        if ([IO.Directory]::Exists($retiredFs)) { throw 'Both live and retired directories exist; refusing ambiguous cleanup' }
        $metadata = ReadState (Join-Path $root '.twi-meta.json')
        foreach ($field in @('app_id', 'app_exe', 'app_title', 'version')) {
            if ($metadata.$field -cne $state.metadata.$field) { throw 'Installation metadata changed; refusing cleanup' }
        }
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        while ($true) {
            try { [IO.Directory]::Move($rootFs, $retiredFs); break }
            catch {
                if ([DateTime]::UtcNow -ge $deadline) { throw }
                Start-Sleep -Milliseconds 200
            }
        }
    }
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($registryPath, $true)
    try { $key.SetValue('InstallLocation', $retired); $key.Flush() } finally { $key.Dispose() }
    $lastError = $null
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        try {
            NoReparse $retired
            if ([IO.Directory]::Exists($retiredFs)) {
                # Preserve metadata and the standalone retry entry through partial deletion.
                foreach ($entry in [IO.Directory]::EnumerateFileSystemEntries($retiredFs)) {
                    if ([IO.Path]::GetFileName($entry) -cne '.twi-meta.json') { DeleteEntry $entry }
                }
            }
            if ($state.metadata.desktop_shortcut) {
                $title = [string]$state.metadata.app_title
                if ($state.metadata.PSObject.Properties.Name -contains 'shortcut_title' -and $null -ne $state.metadata.shortcut_title) { $title = [string]$state.metadata.shortcut_title }
                SafeName $title
                $shortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) "$title.lnk"
                if ([IO.File]::Exists($shortcut) -and (([IO.File]::GetAttributes($shortcut) -band [IO.FileAttributes]::ReparsePoint) -eq 0)) {
                    $shell = New-Object -ComObject WScript.Shell
                    $link = $null
                    try {
                        $link = $shell.CreateShortcut($shortcut)
                        $expectedTarget = $root
                        foreach ($part in $exeParts) { $expectedTarget = Join-Path $expectedTarget $part }
                        if (SamePath $link.TargetPath $expectedTarget) { [IO.File]::Delete($shortcut) }
                    } finally {
                        if ($null -ne $link) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($link) }
                        $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
                    }
                }
            }
            if ([IO.Directory]::Exists($retiredFs)) {
                $metadataPath = Join-Path $retired '.twi-meta.json'
                if ([IO.File]::Exists((FilePath $metadataPath))) { DeleteEntry $metadataPath }
                [IO.Directory]::Delete($retiredFs, $false)
            }
            $lastError = $null
            break
        } catch {
            $lastError = $_
            Start-Sleep -Seconds 1
        }
    }
    if ($null -ne $lastError -or [IO.Directory]::Exists($retiredFs)) { throw "Cleanup could not finish. Retry uninstall from Apps & Features. $lastError" }
    # Remove the blocking marker only after files and owned shortcuts are gone.
    [IO.File]::Delete((FilePath $marker))
    if (RegistryOwned $state) { [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($registryPath, $false) }
    [IO.File]::AppendAllText((FilePath $logPath), "Cleanup completed`r`n")
    $lockStream.Dispose(); $lockStream = $null
    # Only this unique worker directory is removed; the application-data folder is untouched.
    try { DeleteEntry $worker } catch { }
    exit 0
} catch {
    [IO.File]::AppendAllText((FilePath $logPath), "Cleanup failed: $($_.Exception.Message)`r`n")
    exit 1
} finally {
    if ($null -ne $lockStream) { $lockStream.Dispose() }
}
