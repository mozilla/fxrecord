# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Deploy to the given machine.

    .Description
        Deploy the appropriate binaries and management scripts to the given machine.

        SSH is required to be enabled and set up correctly for PowerShell
        sessions. See Scripts/Enable-SSH.ps1.
#>

param(
    [Parameter(Mandatory = $true)]
    [string]$HostName,

    [Parameter(Mandatory = $true)]
    [ValidateSet("runner", "recorder")]
    [string]$MachineType,

    [Parameter(Mandatory = $true)]
    [string]$UserName
)

# Exit script on error.
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSDefaultParameterValues["*:ErrorAction"] = "Stop"

# Ensure we are executing from the repository root.
Set-Location (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Definition))

$session = New-PSSession -UserName $UserName $HostName

try {
    # Compute paths for PowerShell directories on the remote host.
    $remotePaths = Invoke-Command -Session $session -ScriptBlock {
        $paths = @{}
        $paths.PowerShell = Join-Path $home "Documents" "PowerShell"
        $paths.Scripts = Join-Path $paths.PowerShell "Scripts"
        $paths.Modules = Join-Path $paths.PowerShell "Modules"

        New-Item -ItemType Directory -Path $paths.Scripts -Force > $null

        $paths
    }

    Push-Location contrib\deployment

    # Copy management scripts to the machine.
    Write-Host Copying Profile.ps1...
    Copy-Item -Force -Path Profile.ps1 -ToSession $session -Destination $remotePaths.PowerShell

    Write-Host Copying PowerShell Modules...
    Copy-Item -Recurse -Force -Path Modules -ToSession $session -Destination $remotePaths.Modules

    Write-Host Copying PowerShell scripts...
    if ($MachineType -eq "runner") {
        Copy-Item -Force -Path Scripts\fxrunner\*.ps1 -ToSession $session -Destination $remotePaths.Scripts
    }
    else {
        Copy-Item -Force -Path Scripts\fxrecorder\*.ps1 -ToSession $session -Destination $remotePaths.Scripts
    }

    Write-Host Copying configuration...

    $configFile = "config\${HostName}\fxrecord.toml"
    if (Test-Path $configFile) {
        if ($MachineType -eq "runner") {
            Copy-Item -Force -Path $configFile -ToSession $session -Destination C:\fxrunner\
        }
        else {
            Copy-Item -Force -Path $configFile -ToSession $session -Destination C:\fxrecorder\
        }
    }
    else {
        Write-Host No configuration found for host ${HostName}, skipping...
    }

    Pop-Location

    # Build fxrecord and deploy the exectuable.
    Write-Host Building fxrecord...
    cargo build --release

    Write-Host Copying binaries...

    if ($MachineType -eq "runner") {
        New-Item C:\fxrunner -ItemType Directory -Force > $null
        Copy-Item -Force -Path target\release\fxrunner.exe -ToSession $session -Destination C:\fxrunner
    }
    else {
        New-Item C:\fxrecorder -ItemType Directory -Force > $null
        Copy-Item -Force -Path target\release\fxrecorder.exe -ToSession $session -Destination C:\fxrecorder
        Copy-Item -Force -Path vendor -ToSession $session -Destination C:\fxrecorder
        Copy-Item -Force -Path requirements.txt -ToSession $session -Destination C:\fxrecorder
    }

}
finally {
    Remove-PSSession $session
}
