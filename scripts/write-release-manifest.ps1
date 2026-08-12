param(
    [string]$Version = '',
    [string]$Repository = 'bruceblink/alter-sendme',
    [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$output = if ($OutputDirectory) { $OutputDirectory } else { Join-Path $root 'dist' }
$versionValue = if ($Version) { $Version.TrimStart('v') } else { (Get-Content (Join-Path $root 'Cargo.toml') | Select-String '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value }

if (-not $versionValue) {
    throw 'Unable to determine the application version.'
}
if (-not $Repository -or -not ($Repository -match '^[^/]+/[^/]+$')) {
    throw 'Repository must use the owner/name format.'
}

$tag = "v$versionValue"
$portableName = "AlterSendme-$versionValue-windows-portable.zip"
$installerName = "AlterSendme-$versionValue-windows-setup.exe"
$portablePath = Join-Path $output $portableName
$installerPath = Join-Path $output $installerName
$portableShaPath = "$portablePath.sha256"
$installerShaPath = "$installerPath.sha256"

foreach ($path in @($portablePath, $installerPath, $portableShaPath, $installerShaPath)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Release asset is missing: $path"
    }
}

function Read-Sha256Prefix([string]$Path) {
    $line = Get-Content -LiteralPath $Path -TotalCount 1
    $hash = ($line -split '\s+')[0]
    if (-not ($hash -match '^[0-9a-fA-F]{64}$')) {
        throw "Invalid SHA-256 file: $Path"
    }
    return $hash.ToLowerInvariant()
}

$releaseBaseUrl = "https://github.com/$Repository/releases/download/$tag"
$manifest = [ordered]@{
    version = $versionValue
    platforms = [ordered]@{
        'windows-x86_64-nsis' = [ordered]@{
            url = "$releaseBaseUrl/$installerName"
            sha256 = Read-Sha256Prefix $installerShaPath
        }
        'windows-x86_64-portable' = [ordered]@{
            url = "$releaseBaseUrl/$portableName"
            sha256 = Read-Sha256Prefix $portableShaPath
        }
    }
}

New-Item -ItemType Directory -Path $output -Force | Out-Null
$manifestPath = Join-Path $output 'latest.json'
$manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifestPath -Encoding utf8
Write-Host "Created $manifestPath"
