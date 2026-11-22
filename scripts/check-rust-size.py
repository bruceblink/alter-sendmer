#!/usr/bin/env python3
"""检查 Rust 源文件和函数的物理行数上限。"""

from pathlib import Path
import re
import sys


FILE_LIMIT = 600
FUNCTION_LIMIT = 120
FUNCTION_PATTERN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)"
)


def function_spans(lines):
    """按下一个函数声明定位当前函数的源代码范围。"""
    starts = []
    for index, line in enumerate(lines):
        match = FUNCTION_PATTERN.match(line)
        if match:
            starts.append((index, match.group(1)))
    for position, (start, name) in enumerate(starts):
        end = starts[position + 1][0] if position + 1 < len(starts) else len(lines)
        yield name, start + 1, end, end - start


def check_file(path):
    """返回一个源文件和其中超限函数的错误列表。"""
    lines = path.read_text(encoding="utf-8").splitlines()
    errors = []
    if len(lines) > FILE_LIMIT:
        errors.append(f"{path}: {len(lines)} lines exceeds {FILE_LIMIT}")
    for name, start, end, size in function_spans(lines):
        if size > FUNCTION_LIMIT:
            errors.append(f"{path}:{start}: {name} spans {size} lines, exceeds {FUNCTION_LIMIT}")
    return errors


def main():
    """检查 src 下的 Rust 文件，失败时返回非零状态。"""
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("src")
    errors = [error for path in sorted(root.rglob("*.rs")) for error in check_file(path)]
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Rust source size checks passed under {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
