#!/usr/bin/env python3
"""使用项目现有打包器生成产物，并验证安装包内的程序架构。"""

import base64
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile


def run(*args, **options):
    """以独立参数执行命令。失败时终止该目标的打包或验证。"""
    return subprocess.run(args, check=True, **options)


def package(target, output, scratch):
    """在原生工具链上打包，并复用既有重命名和便携包脚本。"""
    platform, architecture = target.split("-", 1)
    triple = os.environ["PACKAGE_RUST_TARGETS"]
    compiler = run("rustc", "-Vv", capture_output=True, text=True).stdout
    if f"host: {triple}" not in compiler.splitlines():
        raise ValueError(f"打包需要 {triple} 原生 Rust 工具链。\n{compiler}")

    formats = {"windows": "nsis", "macos": "app,dmg", "linux": "appimage,deb"}
    run("cargo", "packager", "--release", "--formats", formats[platform], "--out-dir", str(scratch))
    updater_arch = {"arm64": "aarch64", "amd64": "x86_64", "x64": "x86_64"}.get(architecture, architecture)
    run("pwsh", "-NoProfile", "-File", "scripts/rename-release-packages.ps1", "-Target",
        f"{platform}-{updater_arch}", "-Directory", str(scratch))
    stem = os.environ["PACKAGE_STEM"]
    for asset in scratch.glob(f"{stem}.*"):
        shutil.move(asset, output / asset.name)
    for asset in scratch.glob(f"{stem}-setup.exe*"):
        shutil.move(asset, output / asset.name)
    if platform == "windows":
        run("pwsh", "-NoProfile", "-File", "scripts/package-portable.ps1", "-SkipBuild",
            "-Architecture", architecture, "-OutputDirectory", str(output))


def pe_machine(executable):
    """读取 PE 文件头中的实际机器类型。"""
    with executable.open("rb") as stream:
        if stream.read(2) != b"MZ":
            raise ValueError(f"无效的 PE 文件 {executable}")
        stream.seek(0x3C)
        offset, = struct.unpack("<I", stream.read(4))
        stream.seek(offset)
        if stream.read(4) != b"PE\0\0":
            raise ValueError(f"无效的 PE 文件头 {executable}")
        return struct.unpack("<H", stream.read(2))[0]


def verify(target, output, scratch):
    """从实际发布产物提取程序并验证架构，macOS 同时对应用与映像验签。"""
    platform, architecture = target.split("-", 1)
    stem = os.environ["PACKAGE_STEM"]
    if platform == "windows":
        expected = {"x64": 0x8664, "arm64": 0xAA64}[architecture]
        installer = output / f"{stem}-setup.exe"
        sevenzip = shutil.which("7z") or str(Path(os.environ["ProgramFiles"]) / "7-Zip/7z.exe")
        run(sevenzip, "x", "-y", f"-o{scratch / 'installer'}", str(installer), "alter-sendmer.exe")
        with zipfile.ZipFile(output / f"{stem}-portable.zip") as archive:
            if archive.testzip() is not None:
                raise ValueError("便携 ZIP 内容校验失败。")
            portable = Path(archive.extract("AlterSendmer.exe", scratch / "portable"))
        for executable in (portable, scratch / "installer/alter-sendmer.exe"):
            if pe_machine(executable) != expected:
                raise ValueError(f"二进制架构不匹配 {executable}")
    elif platform == "macos":
        dmg = output / f"{stem}.dmg"
        run("hdiutil", "verify", "-quiet", str(dmg))
        run("codesign", "--verify", "--strict", str(dmg))
        run("7zz", "x", "-sns-", "-y", f"-o{scratch / 'dmg'}", str(dmg))
        run("tar", "-xzf", str(output / f"{stem}.app.tar.gz"), "-C", str(scratch))
        for app in (scratch / "AlterSendmer.app", scratch / "dmg/AlterSendmer/AlterSendmer.app"):
            run("codesign", "--verify", "--deep", "--strict", str(app))
            # macOS lipo 的输入文件位于命令参数之前，错误顺序会把文件路径当作架构名。
            run("lipo", str(app / "Contents/MacOS/alter-sendmer"), "-verify_arch", architecture)
    else:
        deb, image = output / f"{stem}.deb", output / f"{stem}.AppImage"
        metadata_arch = run("dpkg-deb", "--field", str(deb), "Architecture", capture_output=True, text=True)
        if metadata_arch.stdout.strip() != architecture:
            raise ValueError(f"DEB 架构不匹配 {deb}")
        run("dpkg-deb", "--extract", str(deb), str(scratch / "deb"))
        run(str(image), "--appimage-extract", cwd=scratch, stdout=subprocess.DEVNULL)
        expected = {"amd64": "Advanced Micro Devices X86-64", "arm64": "AArch64"}[architecture]
        for executable in (image, scratch / "deb/usr/bin/alter-sendmer",
                           scratch / "squashfs-root/usr/bin/alter-sendmer"):
            header = run("readelf", "-h", str(executable), capture_output=True, text=True,
                         env={**os.environ, "LC_ALL": "C"}).stdout
            if not re.search(rf"^\s*Machine:\s+{expected}$", header, re.MULTILINE):
                raise ValueError(f"二进制架构不匹配 {executable}")


def verify_minisign(directory, scratch):
    """用应用内置的公开密钥验签，避免打包密钥与更新客户端不匹配。"""
    key = re.search(r'const UPDATE_PUBLIC_KEY: &str = "([^"]+)";', Path("src/updater.rs").read_text(encoding="utf-8"))
    if not key:
        raise ValueError("找不到应用内置的更新公钥。")
    public_key = scratch / "updater.pub"
    public_key.write_bytes(base64.b64decode(key[1], validate=True))
    signatures = sorted(directory.glob("*.sig"))
    if not signatures:
        raise ValueError("没有可验证的 minisign 签名。")
    for signature in signatures:
        decoded = scratch / "asset.minisig"
        decoded.write_bytes(base64.b64decode(signature.read_bytes().strip(), validate=True))
        run("minisign", "-Vm", str(signature.with_suffix("")), "-x", str(decoded), "-p", str(public_key), "-q")
        print(f"已通过 minisign 验签 {signature.name}")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as temporary:
        scratch = Path(temporary)
        if sys.argv[1] == "minisign":
            verify_minisign(Path(sys.argv[2]), scratch)
        else:
            target, output = os.environ["PACKAGE_TARGET"], Path(os.environ["PACKAGE_OUTPUT_DIR"])
            {"package": package, "verify": verify}[sys.argv[1]](target, output, scratch)
