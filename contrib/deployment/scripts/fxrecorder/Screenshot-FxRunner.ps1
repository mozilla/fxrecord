# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Take a screenshot of the configured fxrunner machine.
#>

param(
    [string]$ConfigPath = "C:\fxrecorder\fxrecord.toml"
)

Import-Module FxRecord-Management

$config = Read-TOML $ConfigPath

ffmpeg `
    -hide_banner `
    -f dshow `
    -i "video=$($config["fxrecorder.recording"]["device"])" `
    -vframes 1 `
    screenshot.jpg
