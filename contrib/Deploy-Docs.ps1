# Any copyright is dedicated to the Public Domain.
# http://creativecommons.org/publicdomain/zero/1.0/

<#
    .Synopsis
        Deploy the docs from the default branch to GitHub pages.
#>
$repoRoot = (Get-Location).Path

function Test-GitBranch([string]$branchName) {
    <#
        .Synopsis
            Check if a git branch exists.
    #>
    $null -ne (git rev-parse --revs-only $branchName)
}

try {
    $branch = (git branch --show-current)
    if ($branch -ne "default") {
        throw "ERROR: This command must be run from the default branch."
    }

    if (Test-GitBranch gh-pages) {
        throw "ERROR: Delete the local gh-pages branch before running this command."
    }

    git switch --quiet --detach default

    Set-Location (Join-Path $repoRoot "docs")

    # Always do a clean build of the docs.
    .\make clean
    .\make html
    if (-not $?) {
        throw "ERROR: sphinx-build failed"
    }

    Set-Location $repoRoot

    # Make sure GitHub doesn't serve our content with Jekyll.
    Write-Output "" > (Join-Path "docs" "build" "html" ".nojekyll")

    git add -f docs\build\html
    git commit --no-gpg-sign -m "Build docs for $(git rev-parse HEAD)"

    # This path must use forward slashes, regardless of platform.
    git subtree split --prefix docs/build/html --branch gh-pages
    git push origin -f gh-pages:gh-pages

    # Cleanup: delete the local gh-pages branch so we can re-run this script
    #          without errors.
    git branch -D gh-pages
}
finally {
    git switch --quiet default
}
