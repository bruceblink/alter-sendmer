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


if __name__ == "__main__":
    unittest.main()
