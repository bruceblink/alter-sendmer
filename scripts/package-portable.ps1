param(
    [ValidateSet('debug', 'release')]
    [string]$Configuration = 'release',
    [string]$Version = '',
    [string]$OutputDirectory = '',
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$targetDirectory = Join-Path $root "target\$Configuration"
$output = if ($OutputDirectory) { $OutputDirectory } else { Join-Path $root 'dist' }
$versionValue = if ($Version) { $Version } else { (Get-Content (Join-Path $root 'Cargo.toml') | Select-String '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value }

if (-not $versionValue) {
    throw 'Unable to determine the application version.'
}

if (-not $SkipBuild) {
    if ($Configuration -eq 'release') {
        cargo build --locked --release --manifest-path (Join-Path $root 'Cargo.toml')
    } else {
        cargo build --locked --manifest-path (Join-Path $root 'Cargo.toml')
    }
}

$executable = Join-Path $targetDirectory 'alter-sendme-gpui.exe'
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build did not produce $executable"
}

$stage = Join-Path $root 'artifacts\portable-input'
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Get-ChildItem -LiteralPath $stage -Force -ErrorAction SilentlyContinue | Remove-Item -Force -Recurse
Copy-Item -LiteralPath $executable -Destination (Join-Path $stage 'AlterSendmer.exe')
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $stage 'README.md')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $stage 'LICENSE')
Copy-Item -LiteralPath (Join-Path $root 'PRIVACY.md') -Destination (Join-Path $stage 'PRIVACY.md')

New-Item -ItemType Directory -Path $output -Force | Out-Null
$archive = Join-Path $output "AlterSendmer-$versionValue-windows-portable.zip"
if (Test-Path -LiteralPath $archive) {
    Remove-Item -LiteralPath $archive -Force
}
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$archive.sha256" -Value "$hash  $(Split-Path -Leaf $archive)" -Encoding ascii
Write-Host "Created $archive"
