#!/usr/bin/env python3
"""Regression tests for the bilingual release note generator."""

import unittest
from unittest.mock import patch

import generate_release_notes as release_notes


class ReleaseDateTests(unittest.TestCase):
    def test_annotated_tag_is_dereferenced_to_its_commit(self) -> None:
        """Annotated tag metadata must never leak into the Markdown date."""
        with patch.object(
            release_notes,
            "git",
            return_value="2026-08-17\n",
        ) as git:
            actual = release_notes.release_date("v0.3.0")

        self.assertEqual(actual, "2026-08-17")
        git.assert_called_once_with(
            "show",
            "-s",
            "--format=%cs",
            "v0.3.0^{commit}",
        )


class ReleaseHighlightsTests(unittest.TestCase):
    def test_reads_only_the_tagged_version(self) -> None:
        """发布正文只附加当前版本的说明，保留旧版本记录供仓库读者查阅。"""
        changelog = "AlterSendmer v0.7.0\n\n- 新增诊断导出。\n\nAlterSendmer v0.6.0\n"
        with patch.object(release_notes, "git", side_effect=["CHANGELOG.md\n", changelog]):
            self.assertEqual(
                release_notes.release_highlights("v0.7.0"),
                "AlterSendmer v0.7.0\n\n- 新增诊断导出。",
            )

    def test_accepts_older_tags_without_a_changelog(self) -> None:
        """历史标签仍可按原流程生成提交范围说明。"""
        with patch.object(release_notes, "git", return_value=""):
            self.assertEqual(release_notes.release_highlights("v0.6.0"), "")


if __name__ == "__main__":
    unittest.main()
