"""Dependency-free checks for the bilingual UI's static contract."""

from html.parser import HTMLParser
from pathlib import Path
import unittest


STATIC = Path(__file__).resolve().parent / "static"


class Markup(HTMLParser):
    def __init__(self):
        super().__init__()
        self.nodes = []

    def handle_starttag(self, tag, attrs):
        self.nodes.append((tag, dict(attrs)))


class LocalizationTests(unittest.TestCase):
    def setUp(self):
        self.markup = Markup()
        self.markup.feed((STATIC / "index.html").read_text(encoding="utf-8"))

    def test_default_is_english_with_exactly_two_language_choices(self):
        self.assertEqual(next(attrs["lang"] for tag, attrs in self.markup.nodes if tag == "html"), "en")
        choices = [attrs for tag, attrs in self.markup.nodes if tag == "option"]
        self.assertEqual([choice["value"] for choice in choices], ["en", "ja"])
        self.assertEqual([choice["value"] for choice in choices if "selected" in choice], ["en"])
        self.assertTrue(any(tag == "label" and attrs.get("for") == "language-select"
                            for tag, attrs in self.markup.nodes))

    def test_static_translation_targets_and_accessibility_have_both_languages(self):
        translations = [attrs for _, attrs in self.markup.nodes if "data-ja" in attrs]
        self.assertGreater(len(translations), 40)
        self.assertTrue(all(attrs["data-ja"].strip() for attrs in translations))
        for _, attrs in self.markup.nodes:
            if "data-ja-label" in attrs:
                self.assertTrue(attrs.get("aria-label"))
                self.assertTrue(attrs["data-ja-label"])
            if "data-ja-content" in attrs:
                self.assertTrue(attrs.get("content"))
                self.assertTrue(attrs["data-ja-content"])

    def test_language_control_does_not_introduce_inline_scripts_or_handlers(self):
        for tag, attrs in self.markup.nodes:
            self.assertFalse(any(name.startswith("on") for name in attrs))
            if tag == "script":
                self.assertEqual(attrs.get("src"), "/app.js")
        ids = [attrs["id"] for _, attrs in self.markup.nodes if "id" in attrs]
        self.assertEqual(len(ids), len(set(ids)))


if __name__ == "__main__":
    unittest.main()
