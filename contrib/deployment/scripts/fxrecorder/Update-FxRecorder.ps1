# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Update FxRecorder after a deploy.

    .Description
        This will ensure the Python dependencies are kept up-to-date.
#>

Import-Module FxRecord-Management

Test-AdminRole

python -m pip install -U pip wheel
python -m pip install --require-hashes -r C:\fxrecorder\requirements.txt
