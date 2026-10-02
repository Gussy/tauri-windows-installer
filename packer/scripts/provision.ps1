# Disposable Windows fixture: keep Windows UAC, Defender and update defaults.
$ErrorActionPreference = 'Stop'
$virtioDrive = (Get-Volume | Where-Object { $_.DriveType -eq 'CD-ROM' -and (Test-Path "$($_.DriveLetter):\guest-agent") } | Select-Object -First 1).DriveLetter
if (-not $virtioDrive) { throw 'VirtIO guest-agent media not found' }
$guestAgentMsi = Get-ChildItem "${virtioDrive}:\guest-agent" -Filter qemu-ga-aarch64.msi -Recurse | Select-Object -First 1
if (-not $guestAgentMsi) { throw 'ARM64 QEMU guest agent missing from VirtIO ISO' }
$install = Start-Process msiexec.exe -ArgumentList '/i', ('"' + $guestAgentMsi.FullName + '"'), '/qn', '/norestart' -Wait -PassThru
if ($install.ExitCode -notin 0,3010) { throw "Guest-agent installation failed: $($install.ExitCode)" }
Set-Service -Name QEMU-GA -StartupType Automatic
Start-Service -Name QEMU-GA

# Packer provisions through its admin account; E2E commands use this distinct
# logged-in standard account through an Interactive/Limited scheduled task.
$password = ConvertTo-SecureString 'TwiTestOnly123!' -AsPlainText -Force
if (-not (Get-LocalUser -Name twi-test -ErrorAction SilentlyContinue)) {
    New-LocalUser -Name twi-test -Password $password -PasswordNeverExpires | Out-Null
}
$users = Get-LocalGroup -SID 'S-1-5-32-545'
Add-LocalGroupMember -Group $users -Member twi-test -ErrorAction SilentlyContinue
$administrators = Get-LocalGroup -SID 'S-1-5-32-544'
Remove-LocalGroupMember -Group $administrators -Member twi-test -ErrorAction SilentlyContinue
$winlogon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Set-ItemProperty $winlogon AutoAdminLogon '1'
Set-ItemProperty $winlogon DefaultUserName 'twi-test'
Set-ItemProperty $winlogon DefaultDomainName $env:COMPUTERNAME
Set-ItemProperty $winlogon DefaultPassword 'TwiTestOnly123!'
Remove-ItemProperty $winlogon AutoLogonCount -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path C:\temp\twi-e2e -Force | Out-Null
& icacls.exe C:\temp /grant 'twi-test:(OI)(CI)M' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not prepare test artifact access' }
if ((Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System').EnableLUA -ne 1) { throw 'Fixture must retain UAC' }
if (-not (Get-MpComputerStatus).RealTimeProtectionEnabled) { throw 'Fixture must retain Defender real-time protection' }
# Do not remove a preinstalled WebView2 runtime. Missing-runtime cases need a
# separate intentionally prepared snapshot rather than security-policy changes.
Write-Host 'Standard-user fixture provisioned with default Windows protections.'
