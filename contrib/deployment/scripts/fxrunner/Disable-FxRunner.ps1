# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Stop and disable fxrunner

    .Description
        Stop and disable the fxrunner scheduled task so the machine can undero maintenance.
        undergo maintenance.
#>

Import-Module FxRecord-Management

Test-AdminRole

Disable-ScheduledTask -TaskPath \fxrecord\ -TaskName fxrunner
Stop-ScheduledTask -TaskPath \fxrecord\ -TaskName fxrunner
