param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('windows-x86_64', 'windows-aarch64', 'linux-x86_64', 'linux-aarch64', 'macos-aarch64')]
    [string]$Target,
    [Parameter(Mandatory = $true)]
    [string]$Directory
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Select-String -LiteralPath (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
if (-not $version) {
    throw 'Unable to determine the application version.'
}

# 按平台匹配 cargo-packager 生成的文件，发布文件名统一包含版本、平台和架构。
$platform = @{
    'windows-x86_64' = 'windows-x64'
    'windows-aarch64' = 'windows-arm64'
    'linux-x86_64' = 'linux-amd64'
    'linux-aarch64' = 'linux-arm64'
    'macos-aarch64' = 'macos-arm64'
}[$Target]
$suffixes = switch -Wildcard ($Target) {
    'windows-*' { '-setup.exe' }
    'linux-*' { '.AppImage', '.deb' }
    'macos-*' { '.dmg', '.app.tar.gz' }
}

foreach ($suffix in $suffixes) {
    $files = @(Get-ChildItem -LiteralPath $Directory -File -Filter "*$suffix")
    if ($files.Count -ne 1) {
        throw "Expected one $Target package ending in $suffix, found $($files.Count)."
    }

    # minisign 验证文件内容。移动安装包时同步移动签名，供自动更新读取。
    $file = $files[0]
    $signature = "$($file.FullName).sig"
    if (-not (Test-Path -LiteralPath $signature -PathType Leaf)) {
        throw "Signature is missing for $($file.Name)."
    }
    $name = "AlterSendmer-$version-$platform$suffix"
    if ($file.Name -cne $name) {
        Rename-Item -LiteralPath $file.FullName -NewName $name
        Rename-Item -LiteralPath $signature -NewName "$name.sig"
    }
}
