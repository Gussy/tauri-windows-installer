# provision.ps1 — Post-install provisioning for Windows 11 ARM test VM
# Run by Packer over WinRM after Windows installation completes.

$ErrorActionPreference = "Stop"

Write-Host "=== Starting VM provisioning ==="

# 1. Install QEMU guest agent from VirtIO ISO
Write-Host "Installing QEMU guest agent..."
$virtioDrive = (Get-Volume | Where-Object { $_.FileSystemLabel -eq "virtio-win*" -or $_.DriveType -eq "CD-ROM" } |
    Where-Object { Test-Path "$($_.DriveLetter):\guest-agent" } |
    Select-Object -First 1).DriveLetter

if ($virtioDrive) {
    $guestAgentMsi = Get-ChildItem "${virtioDrive}:\guest-agent\" -Filter "qemu-ga-aarch64.msi" -Recurse | Select-Object -First 1
    if (-not $guestAgentMsi) {
        $guestAgentMsi = Get-ChildItem "${virtioDrive}:\guest-agent\" -Filter "qemu-ga-*.msi" -Recurse | Select-Object -First 1
    }
    if ($guestAgentMsi) {
        Write-Host "Found guest agent: $($guestAgentMsi.FullName)"
        Start-Process msiexec.exe -ArgumentList "/i `"$($guestAgentMsi.FullName)`" /qn /norestart" -Wait -NoNewWindow
        Write-Host "QEMU guest agent installed."
    } else {
        Write-Warning "QEMU guest agent MSI not found on VirtIO drive."
    }
} else {
    Write-Warning "VirtIO drive not found. Skipping guest agent installation."
}

# 2. Set guest agent service to auto-start
Write-Host "Configuring QEMU guest agent service..."
$svc = Get-Service -Name "QEMU-GA" -ErrorAction SilentlyContinue
if ($svc) {
    Set-Service -Name "QEMU-GA" -StartupType Automatic
    Start-Service -Name "QEMU-GA" -ErrorAction SilentlyContinue
    Write-Host "QEMU guest agent service configured."
} else {
    Write-Warning "QEMU-GA service not found."
}

# 3. Disable Windows Update service
Write-Host "Disabling Windows Update..."
Stop-Service -Name "wuauserv" -Force -ErrorAction SilentlyContinue
Set-Service -Name "wuauserv" -StartupType Disabled
Write-Host "Windows Update disabled."

# 4. Disable Windows Defender real-time protection
Write-Host "Disabling Windows Defender real-time protection..."
try {
    Set-MpPreference -DisableRealtimeMonitoring $true
    Write-Host "Windows Defender real-time protection disabled."
} catch {
    Write-Warning "Could not disable Windows Defender: $_"
}

# 5. Set PowerShell execution policy
Write-Host "Setting PowerShell execution policy..."
Set-ExecutionPolicy RemoteSigned -Force -Scope LocalMachine
Write-Host "Execution policy set to RemoteSigned."

# 6. Disable UAC prompts
Write-Host "Disabling UAC prompts..."
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "EnableLUA" -Value 0 -Type DWord
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "ConsentPromptBehaviorAdmin" -Value 0 -Type DWord
Write-Host "UAC disabled."

# 7. Do NOT install WebView2 — we want to test the installer's bundling

# 8. Create temp directory for test artifacts
Write-Host "Creating temp directory..."
New-Item -ItemType Directory -Path "C:\temp" -Force | Out-Null
Write-Host "C:\temp created."

# 9. Clean temp files and optimize disk
Write-Host "Cleaning up..."
Remove-Item -Path "$env:TEMP\*" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "C:\Windows\Temp\*" -Recurse -Force -ErrorAction SilentlyContinue

# Clear Windows Update cache
Remove-Item -Path "C:\Windows\SoftwareDistribution\Download\*" -Recurse -Force -ErrorAction SilentlyContinue

# Compact OS (reduces disk image size)
Write-Host "Compacting OS..."
try {
    Compact /CompactOS:always 2>$null
} catch {
    Write-Warning "OS compaction skipped."
}

Write-Host "=== Provisioning complete ==="
