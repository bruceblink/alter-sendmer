#!/usr/bin/env python3
"""运行项目的打包命令，检查产物名称、目标和校验文件。"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
from typing import NamedTuple


class Target(NamedTuple):
    """记录打包主机、Rust 编译目标和目标环境的运行方式。"""

    runner: str
    host_os: str
    host_arch: str
    rust_targets: tuple[str, ...]
    execution: str = "native"


# 主机架构和产物架构分别记录。扩展目标由项目脚本提供交叉工具链和测试环境。
TARGETS = {
    "windows-x64": Target("windows-2025", "Windows", "X64", ("x86_64-pc-windows-msvc",)),
    "windows-arm64": Target("windows-11-arm", "Windows", "ARM64", ("aarch64-pc-windows-msvc",)),
    "windows-x86": Target("windows-2025", "Windows", "X64", ("i686-pc-windows-msvc",), "wow64"),
    "macos-arm64": Target("macos-26", "macOS", "ARM64", ("aarch64-apple-darwin",)),
    "linux-amd64": Target("ubuntu-24.04", "Linux", "X64", ("x86_64-unknown-linux-gnu",)),
    "linux-arm64": Target("ubuntu-24.04-arm", "Linux", "ARM64", ("aarch64-unknown-linux-gnu",)),
    "linux-i386": Target("ubuntu-24.04", "Linux", "X64", ("i686-unknown-linux-gnu",), "multilib"),
    "linux-armhf": Target("ubuntu-24.04", "Linux", "X64", ("armv7-unknown-linux-gnueabihf",), "qemu"),
    "linux-riscv64": Target("ubuntu-24.04", "Linux", "X64", ("riscv64gc-unknown-linux-gnu",), "qemu"),
    "linux-ppc64le": Target("ubuntu-24.04", "Linux", "X64", ("powerpc64le-unknown-linux-gnu",), "qemu"),
    "linux-s390x": Target("ubuntu-24.04", "Linux", "X64", ("s390x-unknown-linux-gnu",), "qemu"),
    "linux-loong64": Target("ubuntu-24.04", "Linux", "X64", ("loongarch64-unknown-linux-gnu",), "qemu"),
    "freebsd-amd64": Target("ubuntu-24.04", "Linux", "X64", ("x86_64-unknown-freebsd",), "vm"),
    "freebsd-arm64": Target("ubuntu-24.04", "Linux", "X64", ("aarch64-unknown-freebsd",), "vm"),
}
FORMATS = {
    "windows": {"setup.exe", "portable.zip", "msi", "msix"},
    "macos": {"dmg", "pkg", "app.tar.gz"},
    "linux": {"AppImage", "deb", "rpm", "tar.gz", "flatpak", "snap"},
    "freebsd": {"pkg", "tar.gz"},
}
SIDECARS = (".sig", ".sha256", ".sigstore.json")
CONFIG = Path(".github/desktop-packaging.json")


def load_config():
    """读一次项目配置，并在分配执行器前验证版本、格式和命令。"""
    config = json.loads(CONFIG.read_text(encoding="utf-8"))
    product = config["product"]
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9-]*", product):
        raise ValueError("product 须使用无空格的文件名前缀。")

    source = config["version_source"]
    path = Path(source["path"])
    if not path.resolve().is_relative_to(Path.cwd().resolve()):
        raise ValueError("版本文件须位于当前仓库。")
    if path.suffix == ".toml":
        values = tomllib.loads(path.read_text(encoding="utf-8"))
    elif path.suffix == ".json":
        values = json.loads(path.read_text(encoding="utf-8"))
    else:
        raise ValueError("版本文件须使用 TOML 或 JSON。")
    version = values
    for key in source["key"].split("."):
        version = version[key]
    if not isinstance(version, str) or not re.fullmatch(r"(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*)){2}", version):
        raise ValueError("版本文件须提供 MAJOR.MINOR.PATCH。")
    if os.environ.get("GITHUB_REF_TYPE") == "tag" and os.environ.get("GITHUB_REF_NAME") != f"v{version}":
        raise ValueError(f"打包标签须与版本文件一致，预期 v{version}。")

    targets = config["targets"]
    if not isinstance(targets, dict) or not targets:
        raise ValueError("至少配置一个目标。")
    for target_id, recipe in targets.items():
        if target_id not in TARGETS:
            raise ValueError(f"未知目标 {target_id}。")
        platform = target_id.split("-", 1)[0]
        formats = recipe["formats"]
        if not isinstance(formats, list) or not formats or len(formats) != len(set(formats)):
            raise ValueError(f"{target_id} 的 formats 须为非空列表，且不可重复。")
        if set(formats) - FORMATS[platform]:
            raise ValueError(f"{target_id} 使用了不支持的格式。")
        required_signatures = recipe.get("minisign", [])
        if not isinstance(required_signatures, list) or set(required_signatures) - set(formats):
            raise ValueError(f"{target_id} 的 minisign 须列出已声明的格式。")
        for key in ("package", "verify"):
            command = recipe[key]
            if not isinstance(command, list) or not command or not all(
                isinstance(arg, str) and arg for arg in command
            ):
                raise ValueError(f"{target_id} 的 {key} 须为非空命令参数列表。")
    return product, version, targets


def package_name(stem, kind):
    """生成供下载的文件名，安装器和便携包使用用途后缀。"""
    separator = "-" if kind in {"setup.exe", "portable.zip"} else "."
    return f"{stem}{separator}{kind}"


def verify_files(directory, stem, recipe):
    """检查应有产物、配套文件及单文件校验值，拒绝遗漏或混入其他目标。"""
    required = {package_name(stem, kind) for kind in recipe["formats"]}
    signatures = {f"{package_name(stem, kind)}.sig" for kind in recipe.get("minisign", [])}
    entries = {path.name: path for path in directory.iterdir()}
    missing = (required | signatures) - entries.keys()
    allowed = required | {f"{name}{suffix}" for name in required for suffix in SIDECARS}
    extras = entries.keys() - allowed
    if missing or extras:
        raise ValueError(f"{directory.name} 缺少 {sorted(missing)}，多出 {sorted(extras)}。")
    for path in entries.values():
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"产物须为非空普通文件 {path.name}。")
    for name in sorted(required):
        sidecar = entries.get(f"{name}.sha256")
        if sidecar:
            fields = sidecar.read_text(encoding="utf-8").split()
            if len(fields) != 2 or fields[1].removeprefix("*") != name or fields[0].lower() != sha256(entries[name]):
                raise ValueError(f"单文件校验失败 {sidecar.name}。")
    return entries


def sha256(path):
    """以流式读取计算校验值。"""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def run_package(target_id, output, product, version, recipe):
    """在目标指定的主机上调用项目脚本，按顺序打包、验收和核对产物。"""
    target = TARGETS[target_id]
    if os.environ.get("GITHUB_ACTIONS") == "true" and (
        os.environ.get("RUNNER_OS"), os.environ.get("RUNNER_ARCH")
    ) != (target.host_os, target.host_arch):
        raise ValueError(f"{target_id} 需要 {target.host_os}/{target.host_arch} 打包执行器。")
    stem = f"{product}-{version}-{target_id}"
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    env = dict(
        os.environ,
        PACKAGE_TARGET=target_id,
        PACKAGE_VERSION=version,
        PACKAGE_STEM=stem,
        PACKAGE_OUTPUT_DIR=str(output),
        PACKAGE_FORMATS=",".join(recipe["formats"]),
        PACKAGE_RUST_TARGETS=",".join(target.rust_targets),
        PACKAGE_EXECUTION=target.execution,
    )
    subprocess.run(recipe["package"], env=env, check=True)
    verify_files(output, stem, recipe)
    subprocess.run(recipe["verify"], env=env, check=True)
    verify_files(output, stem, recipe)


def collect(source, output, product, version, targets):
    """从已通过验收的分目标目录汇总发布包，并记录源提交和 SHA-256。"""
    expected = {f"desktop-package-{target_id}" for target_id in targets}
    if {path.name for path in source.iterdir()} != expected:
        raise ValueError("下载的产物目录与打包目标清单不一致。")

    records = []
    for target_id, recipe in sorted(targets.items()):
        directory = source / f"desktop-package-{target_id}"
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"无效的目标目录 {directory}。")
        files = verify_files(directory, f"{product}-{version}-{target_id}", recipe)
        records.extend((target_id, path) for _, path in sorted(files.items()))

    output.mkdir(parents=True, exist_ok=False)
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    index = {"product": product, "version": version, "commit": commit, "files": []}
    sums = []
    for target_id, path in records:
        destination = output / path.name
        with path.open("rb") as source_file, destination.open("xb") as output_file:
            shutil.copyfileobj(source_file, output_file)
        digest = sha256(destination)
        index["files"].append(
            {"target": target_id, "name": path.name, "bytes": destination.stat().st_size, "sha256": digest}
        )
        sums.append(f"{digest}  {path.name}\n")
    index_file = output / "package-index.json"
    index_file.write_text(json.dumps(index, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    sums.append(f"{sha256(index_file)}  {index_file.name}\n")
    (output / "SHA256SUMS").write_text("".join(sums), encoding="utf-8", newline="\n")


def main():
    """提供 CI 和本地验收共用的入口。"""
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="phase", required=True)
    subcommands.add_parser("plan")
    package = subcommands.add_parser("package")
    package.add_argument("--target", required=True)
    package.add_argument("--output", type=Path, required=True)
    bundle = subcommands.add_parser("collect")
    bundle.add_argument("--input", type=Path, required=True)
    bundle.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        product, version, targets = load_config()
        if args.phase == "plan":
            # 当前项目的六个目标各使用一个原生 Rust 工具链。
            matrix = {"include": [
                {"target": key, "runner": TARGETS[key].runner, "rust_target": TARGETS[key].rust_targets[0]}
                for key in targets
            ]}
            encoded = json.dumps(matrix, separators=(",", ":"))
            if destination := os.environ.get("GITHUB_OUTPUT"):
                with Path(destination).open("a", encoding="utf-8", newline="\n") as stream:
                    stream.write(f"matrix={encoded}\n")
                    stream.write(f"version={version}\n")
            print(encoded)
        elif args.phase == "package":
            run_package(args.target, args.output, product, version, targets[args.target])
        else:
            collect(args.input, args.output, product, version, targets)
    except (KeyError, OSError, TypeError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"打包失败：{error}\n")


if __name__ == "__main__":
    main()
