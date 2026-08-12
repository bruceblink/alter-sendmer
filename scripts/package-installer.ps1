param(
    [string]$Version = '',
    [string]$OutputDirectory = '',
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$versionValue = if ($Version) { $Version } else { (Get-Content (Join-Path $root 'Cargo.toml') | Select-String '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value }
$output = if ($OutputDirectory) { $OutputDirectory } else { Join-Path $root 'dist' }
$executable = Join-Path $root 'target\release\alter-sendme-gpui.exe'

if (-not $SkipBuild -and -not (Test-Path -LiteralPath $executable)) {
    cargo build --locked --release --manifest-path (Join-Path $root 'Cargo.toml')
}
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Release executable not found at $executable"
}

$iscc = @(
    (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source,
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $iscc) {
    throw 'Inno Setup 6 (ISCC.exe) is required to build the installer.'
}

$stage = Join-Path $root 'artifacts\installer-input'
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Get-ChildItem -LiteralPath $stage -Force -ErrorAction SilentlyContinue | Remove-Item -Force -Recurse
Copy-Item -LiteralPath $executable -Destination (Join-Path $stage 'AlterSendme.exe') -Force
New-Item -ItemType Directory -Path $output -Force | Out-Null

& $iscc "/DMyAppVersion=$versionValue" "/O$output" (Join-Path $root 'installer\alter-sendme.iss')
$installer = Join-Path $output "AlterSendme-$versionValue-windows-setup.exe"
if (-not (Test-Path -LiteralPath $installer)) {
    throw "Inno Setup did not produce $installer"
}
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$installer.sha256" -Value "$hash  $(Split-Path -Leaf $installer)" -Encoding ascii
Write-Host "Created $installer"
