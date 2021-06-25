# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Start and enable the fxrecorder worker.
#>

Import-Module FxRecord-Management

Test-AdminRole

Set-Service -Name "Generic Worker" -StartupType Automatic -Status Running
