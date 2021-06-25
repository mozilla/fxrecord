# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

function Get-UserSessions {
    <#
        .Synopsis
            Return all active user sessions
        .Outputs
            An array of user session objects.
    #>

    # Sample output of quser:
    # ```
    # USERNAME              SESSIONNAME        ID  STATE   IDLE TIME  LOGON TIME
    # >fxrecorder            rdp-tcp#7           5  Active          .  2021-06-11 6:06 PM
    # ```

    $output = quser 2>&1                           <# Query all user sessions #> `

    if ($global:LASTEXITCODE -eq 1) {
        return @()
    }

    return $output |
        Select-Object -Skip 1 |               <# Strip off the leading column names #> `
        Foreach-Object { $_.substring(1) } |  <# Strip off the leading character (> or space) #>`
        ConvertFrom-String -PropertyNames @("UserName", "SessionName", "Id", "State", "IdleTime", "LoginTime")
}

function Read-TOML {
    <#
    .Synopsis
        Read a TOML file into a table. Does not support nested objects.

    .Outputs
        A hashtable parsed from the the TOML file.
    #>

    param (
        [Parameter(Mandatory = $true)]
        [string]$FileName
    )

    $toml = @{}
    $section = ""

    switch -regex -file $FileName {
        "^\[(.+)\]$" {
            $section = $matches[1]
            $toml[$section] = @{}
        }

        "\s*(.+?)\s*=\s*(.+)" {
            $name, $value = $matches[1..2]

            # Unquote string values
            if (($value[0] -eq '"') -and ($value[-1] -eq '"')) {
                $value = $value.Substring(1, $value.Length - 2)
            }

            $toml[$section][$name] = $value
        }
    }

    $toml
}

function Test-AdminRole {
    <#
        .Synopsis
            Check if the user is in an administrator role.
    #>
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal] $identity
    $adminRole = [Security.Principal.WindowsBuiltInRole]::Administrator

    if (!$principal.IsInRole($adminRole)) {
        Write-Host -ForegroundColor Red "$($MyInvocation.ScriptName): Admin required"
        exit 1
    }

}

Export-ModuleMember Get-UserSessions
Export-ModuleMember Read-TOML
Export-ModuleMember Test-AdminRole
