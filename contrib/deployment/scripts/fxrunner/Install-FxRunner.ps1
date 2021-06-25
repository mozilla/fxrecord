# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Set up this machine to be an fxrunner host.
#>

Import-Module FxRecord-Management

Test-AdminRole

$WINLOGON_KEY = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\"

$credentials = Get-Credential $env:USERNAME

Set-ItemProperty `
    -Path $WINLOGON_KEY `
    -Name "DefaultUserName" `
    -Type String `
    -Value $credentials.UserName

Set-ItemProperty `
    -Path $WINLOGON_KEY `
    -Name "DefaultPassword" `
    -Type String `
    -Value (ConvertFrom-SecureString -AsPlainText $credentials.Password)

Set-ItemProperty `
    -Path $WINLOGON_KEY `
    -Name "AutoAdminLogon" `
    -Type Dword `
    -Value 1

New-Item -ItemType Directory -Path C:\fxrunner

$action = New-ScheduledTaskAction `
    -Execute "C:\fxrunner\fxrunner.exe" `
    -WorkingDirectory "C:\fxrunner"

$trigger = New-ScheduledTaskTrigger `
    -AtLogon `
    -User $env:USERNAME

$principal = New-ScheduledTaskPrincipal $env:USERNAME

$settings = New-ScheduledTaskSettingsSet `
    -ExecutionTimelimit (New-TimeSpan) <# Do not impose a time limit on execution (default 3d) #>

$task = New-ScheduledTask -Action $action -Principal $principal -Trigger $trigger -Settings $settings

Register-ScheduledTask -TaskPath "\fxrecord\" -TaskName fxrunner -InputObject $task

# Enable legacy IO counters
diskperf -Y

# Disable services that impact performance or are not required.

# Disable "SysMain" (superfetch)
Set-Service -Name SysMain -StartupType Disabled -Status Stopped

# Disable "Synaptics Touchpad Enhanced Service"
# We ignore errors in case the reference hardware does not have this service.
Set-Service -Name SynTPEnhService -StartupDype Disabled -Status -Stopped -ErrorAction Ignore

# Disable "Connected User Experiences and Telemetry"
Set-Service -Name DiagTrack -StartupType Disabled -Status Stopped

# Disable "Touch Keyboard and Handwriting Panel Service"
Set-Service -Name TabletInputService -StartupType Disabled -Status Stopped

# Disable "Windows Search"
Set-Service -Name WSearch -StartupType Disabled -Status Stopped

# Disable "Print Spooler"
Set-Service -Name Spooler -StartupType Disabled -Status Stopped

# Disable "Microsoft Compatability Appraiser"
Disable-ScheduledTask `
    -TaskPath "\Microsoft\Windows\Application Experience\" `
    -TaskName "Microsoft Compatibility Appraiser"
