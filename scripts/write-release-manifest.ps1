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

# 使用标准下载名称查找签名，更新协议继续使用 Rust 的架构标识。
function Resolve-UpdateAsset {
    param(
        [Parameter(Mandatory = $true)] [string]$Suffix,
        [Parameter(Mandatory = $true)] [string]$Format
    )

    $name = "AlterSendmer-$versionValue-$Suffix"
    $results = @(Get-ChildItem -LiteralPath $artifacts -Recurse -File -Filter $name)
    if ($results.Count -ne 1) {
        throw "Expected one $name asset, found $($results.Count)."
    }

    $asset = $results[0]
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
        'windows-x86_64' = Resolve-UpdateAsset 'windows-x64-setup.exe' 'nsis'
        'windows-aarch64' = Resolve-UpdateAsset 'windows-arm64-setup.exe' 'nsis'
        'linux-x86_64' = Resolve-UpdateAsset 'linux-amd64.AppImage' 'appimage'
        'linux-aarch64' = Resolve-UpdateAsset 'linux-arm64.AppImage' 'appimage'
        'macos-aarch64' = Resolve-UpdateAsset 'macos-arm64.app.tar.gz' 'app'
    }
}

$manifestDirectory = Split-Path -Parent $manifestPath
New-Item -ItemType Directory -Path $manifestDirectory -Force | Out-Null
$utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 6), $utf8WithoutBom)

$checksumPath = Join-Path $manifestDirectory 'SHA256SUMS'
$releaseFiles = @(
    Get-ChildItem -LiteralPath $artifacts -Recurse -File |
        Where-Object { $_.Name -ne 'SHA256SUMS' } |
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
