# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Stop and disable fxrecorder.

    .Description
        Stop and disable the Taskcluster generic worker service so the host can
        undergo maintenance.

        Any active task users will be logged off and their processes terminated.

        Home directories are not removed.
#>

Import-Module FxRecord-Management

Test-AdminRole

$WINLOGON_KEY = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\"

Set-Service -Name "Generic Worker" -StartupType Disabled -Status Stopped

# Prevent auto-login on next restart.
Remove-Item -Force "C:\generic-worker\next-task-user.json" -ErrorAction Ignore
Remove-ItemProperty $WINLOGON_KEY -Name "DefaultUserName" -ErrorAction Ignore
Remove-ItemProperty $WINLOGON_KEY -Name "DefaultPassword" -ErrorAction Ignore

# Log off existing task users and kill any remaining processes
foreach ($session in (Get-UserSessions | Where-Object { $_.UserName -like "task_*" })) {
    logoff $session.Id
}

# Kill any remaining task processes.
Get-Process -IncludeUserName | Where-Object { $_.Username -like "task_*" } | Stop-Process

# Delete any task users.
$taskUsers = Get-LocalUser | Where-Object { $_.Name -like "task_*" }
Remove-LocalUser $taskUsers
