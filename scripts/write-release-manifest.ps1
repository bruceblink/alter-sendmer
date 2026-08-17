param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$Repository = 'bruceblink/alter-sendmer',
    [string]$ArtifactsDirectory = '',
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$artifacts = if ($ArtifactsDirectory) { $ArtifactsDirectory } else { Join-Path $root 'dist' }
$manifestPath = if ($OutputPath) { $OutputPath } else { Join-Path $artifacts 'latest.json' }
$versionValue = $Version.Trim().TrimStart('v')

if (-not ($versionValue -match '^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$')) {
    throw "Version must be a semantic version: $Version"
}
if (-not ($Repository -match '^[^/]+/[^/]+$')) {
    throw 'Repository must use the owner/name format.'
}
if (-not (Test-Path -LiteralPath $artifacts -PathType Container)) {
    throw "Artifacts directory does not exist: $artifacts"
}

# Finds the single signed installer for one updater target and returns its manifest entry.
function Resolve-UpdateAsset {
    param(
        [Parameter(Mandatory = $true)] [string]$TargetFolder,
        [Parameter(Mandatory = $true)] [string]$Pattern,
        [Parameter(Mandatory = $true)] [string]$Format
    )

    $matches = @(
        Get-ChildItem -LiteralPath $artifacts -Recurse -File |
            Where-Object {
                $_.FullName -like "*$TargetFolder*" -and $_.Name -like $Pattern
            }
    )
    if ($matches.Count -ne 1) {
        throw "Expected one $TargetFolder asset matching $Pattern, found $($matches.Count)."
    }

    $asset = $matches[0]
    $signaturePath = "$($asset.FullName).sig"
    if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
        throw "Signature is missing for $($asset.Name): $signaturePath"
    }
    $signature = (Get-Content -LiteralPath $signaturePath -Raw).Trim()
    if (-not $signature) {
        throw "Signature is empty for $($asset.Name)."
    }

    return [ordered]@{
        url = "https://github.com/$Repository/releases/download/v$versionValue/$($asset.Name)"
        signature = $signature
        format = $Format
    }
}

$manifest = [ordered]@{
    version = $versionValue
    notes = "AlterSendmer v$versionValue"
    pub_date = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{
        'windows-x86_64' = Resolve-UpdateAsset 'windows-x86_64' '*_x64-setup.exe' 'nsis'
        'linux-x86_64' = Resolve-UpdateAsset 'linux-x86_64' '*.AppImage' 'appimage'
        'macos-aarch64' = Resolve-UpdateAsset 'macos-aarch64' '*.app.tar.gz' 'app'
    }
}

$manifestDirectory = Split-Path -Parent $manifestPath
New-Item -ItemType Directory -Path $manifestDirectory -Force | Out-Null
$utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 6), $utf8WithoutBom)

$checksumPath = Join-Path $manifestDirectory 'SHA256SUMS'
$releaseFiles = @(
    Get-ChildItem -LiteralPath $artifacts -Recurse -File |
        Where-Object {
            $_.Extension -ne '.sig' -and
            $_.Name -ne 'SHA256SUMS' -and
            $_.Name -notlike '*.sha256'
        } |
        Sort-Object Name
)
$duplicateNames = $releaseFiles | Group-Object Name | Where-Object Count -gt 1
if ($duplicateNames) {
    throw "Release asset names must be unique: $($duplicateNames.Name -join ', ')"
}
$checksumLines = $releaseFiles | ForEach-Object {
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
    "$hash  $($_.Name)"
}
[IO.File]::WriteAllLines($checksumPath, $checksumLines, $utf8WithoutBom)

Write-Host "Created $manifestPath"
Write-Host "Created $checksumPath"
