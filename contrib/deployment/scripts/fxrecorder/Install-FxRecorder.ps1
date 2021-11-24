# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Set up this machine to be an fxrecorder host.

    .Description
        This will install most dependencies. It currently does not install
        ImageMagick or ffmpeg. See the installation documenation for details.
#>

Import-Module FxRecord-Management

Test-AdminRole

$TASKCLUSTER_VERSION = "v44.0.0"
$NSSM_VERSION = "2.24"
$PYTHON_VERSION = "3.9.9"

Set-ExecutionPolicy Unrestricted -Force -Scope Process

$webClient = [System.Net.WebClient]::new()

$webClient.DownloadFile("https://www.nssm.cc/release/nssm-${NSSM_VERSION}.zip", "C:\nssm.zip")
Expand-Archive -LiteralPath C:\nssm.zip -DestinationPath C:\
Rename-Item C:\nssm-2.24 c:\nssm
Remove-Item c:\nssm-2.24.zip
$nssm = "C:\nssm\win64\nssm.exe"

# Install generic-worker and its dependencies.
New-Item -ItemType Directory C:\generic-worker

$genericWorker = "C:\generic-worker\generic-worker.exe"
$livelog = "C:\generic-worker\livelog.exe"
$taskclusterProxy = "C:\generic-worker\taskcluster-proxy.exe"

$webClient.DownloadFile(
    "https://github.com/taskcluster/taskcluster/releases/download/v${TASKCLUSTER_VERSION}/generic-worker-multiuser-windows-amd64",
    $genericWorker)

$webClient.DownloadFile(
    "https://github.com/taskcluster/taskcluster/releases/download/v${TASKCLUSTER_VERSION}/livelog-windows-amd64",
    $livelog)

$webClient.DownloadFile(
    "https://github.com/taskcluster/taskcluster/releases/download/v${TASKCLUSTER_VERSION}/taskcluster-proxy-windows-amd64",
    $taskclusterProxy)

$SIGNING_KEY = C:\generic-worker\generic-worker-ed25519-signing-key.key
$CONFIG_FILE = C:\generic-worker\generic-worker-config.json

# Generate a keypair for signing artifacts.
& $genericWorker new-ed25519-keypair --file $SIGNING_KEY

$accessToken = Read-Host "Access Token"
$clientId = Read-Host "Client ID"
$workerId = Read-Host "Worker ID"

Set-Content -Path $CONFIG_FILE @"
{
    "accessToken": "$accessToken",
    "clientId": "$clientId",
    "cleanUpTaskDirs": true,
    "ed25519SigningKeyLocation": "$SIGNING_KEY",
    "workerId": "$workerId",
    "provisionerId": "performance-hardware",
    "workerType": "gecko-t-fxrecorder",
    "rootURL": "https://firefox-ci-tc.services.mozilla.com",
    "shutdownMachineOnIdle": false,
    "shutdownMachineOnError": false,
    "livelogExecutable": "$($livelog.Replace("\", "\\"))",
    "taskclusterProxyExecutable": "$($taskclusterProxy.Replace("\", "\\"))",
    "wstAudience": "firefoxcitc",
    "wstServerURL": "https://firefoxci-websocktunnel.services.mozilla.com/"
}
"@

# Install the generic worker service.
& $nssm install "Generic Worker" c:\generic-worker\generic-worker.exe
& $nssm set "Generic Worker" AppParameters run --config $CONFIG_FILE
& $nssm set "Generic Worker" DisplayName "Generic Worker"
& $nssm set "Generic Worker" Description "Taskcluster worker"
& $nssm set "Generic Worker" Start SERVICE_AUTO_START
& $nssm set "Generic Worker" Type SERVICE_WIN32_OWN_PROCESS
& $nssm set "Generic Worker" AppNoConsole 1
& $nssm set "Generic Worker" AppAffinity All
& $nssm set "Generic Worker" AppStopMethodSkip 0
& $nssm set "Generic Worker" AppExit Default Exit
& $nssm set "Generic Worker" AppRestartDelay 0
& $nssm set "Generic Worker" AppStdout C:\generic-worker\generic-worker-service.log
& $nssm set "Generic Worker" AppStderr c:\generic-worker\generic-worker-service.log
& $nssm set "Generic Worker" AppRotateFiles 1
& $nssm set "Generic Worker" AppRotateSeconds 3600
& $nssm set "Generic Worker" AppRotateBytes 0

# Download and install Python
$pyInstaller = "C:\python-installer.exe"
$pythonDir = "C:\python-${PYTHON_VERSION}"

$webClient.DownloadFile("https://www.python.org/ftp/python/${PYTHON_VERSION}/python-${PYTHON_VERSION}-amd64.exe", $pyInstaller)
& $pyInstaller /quiet InstallAllUsers=1 PrependPath=1 Include_test=0 TargetDir=$pythonDir
Remove-Item $pyInstaller

# Install Python dependencies.
# C:\fxrecorder\requirements.txt is deployed by Deploy.ps1

$env:PATH += ";${pythonDir}"
python -m pip install -U pip wheel
python -m pip install --require-hashes -r C:\fxrecorder\requirements.txt
