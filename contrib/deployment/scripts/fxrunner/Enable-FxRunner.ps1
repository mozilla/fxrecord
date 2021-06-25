
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Enable and start fxrunner.


    .Description
        Enable and start the fxrunner scheduled task so the machine can undero maintenance.
        undergo maintenance.
#>

Import-Module FxRecord-Management

Test-AdminRole

Enable-ScheduledTask -TaskPath \fxrecord\ -TaskName fxrunner
Start-ScheduledTask -TaskPath \fxrecord\ -TaskName fxrunner
