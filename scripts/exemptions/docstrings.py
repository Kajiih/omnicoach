"""Exemption mutations for Batch D: docstring rules."""

from __future__ import annotations

from .model import Cluster, Mutation, Unmutated

VERB_LED_RETURNS_SECTION = "src/code_lint/rules/verb_led_returns_section.rs"

CLUSTER = Cluster(
    rules=("verb_led_returns_section",),
    mutations=(
        Mutation(
            label="bare Return/Yield + noun read as a noun phrase (verb-led-returns-section)",
            path=VERB_LED_RETURNS_SECTION,
            edits=(
                (
                    "    let is_noun_phrase =\n"
                    "        NOUN_STEMS.contains(&lowercase.as_str()) && NOUN_FOLLOWERS.contains(&next_word.as_str());",
                    "    let _ = (NOUN_STEMS, NOUN_FOLLOWERS, next_word);\n    let is_noun_phrase = false;",
                ),
            ),
            cases=("verb_led_returns_section::pass::return_noun_phrase_exempt",),
        ),
        Mutation(
            label="whole-word match of the opening verb (verb-led-returns-section)",
            path=VERB_LED_RETURNS_SECTION,
            edits=(
                (
                    "    if !SECTION_VERBS.contains(&lowercase.as_str()) {",
                    "    if !SECTION_VERBS.iter().any(|verb| lowercase.starts_with(verb)) {",
                ),
            ),
            cases=("verb_led_returns_section::pass::longer_word_starting_with_verb_exempt",),
        ),
        Mutation(
            label="only the first body line is read (verb-led-returns-section)",
            path=VERB_LED_RETURNS_SECTION,
            edits=(
                (
                    "let Some(opening_line) = section.body.first() else {",
                    "let Some(opening_line) = section.body.last() else {",
                ),
            ),
            cases=("verb_led_returns_section::pass::only_first_body_line_checked",),
        ),
        Mutation(
            label="only the first word of the opening line is read (verb-led-returns-section)",
            path=VERB_LED_RETURNS_SECTION,
            edits=(
                (
                    "    let word = leading_word(text);",
                    "    let word = text\n"
                    "        .split_whitespace()\n"
                    "        .map(leading_word)\n"
                    "        .find(|candidate| SECTION_VERBS.contains(&candidate.to_ascii_lowercase().as_str()))\n"
                    "        .unwrap_or_default();",
                ),
            ),
            cases=("verb_led_returns_section::pass::only_first_word_checked",),
        ),
        Mutation(
            label="only Returns/Yields sections are read (verb-led-returns-section)",
            path=VERB_LED_RETURNS_SECTION,
            edits=(
                (
                    "                DocstringSectionKind::Returns | DocstringSectionKind::Yields\n",
                    "                _\n",
                ),
            ),
            cases=("verb_led_returns_section::pass::other_sections_not_checked",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts (verb-led-returns-section)",
            reason="No exemption: sections that open with a noun phrase.",
            cases=("verb_led_returns_section::pass::noun_phrase_sections",),
        ),
        Unmutated(
            label="constructs the rule never visits (verb-led-returns-section)",
            reason="The summary line is not a section: the rule only reads `PythonDocstring::sections()`.",
            cases=("verb_led_returns_section::pass::summary_line_not_checked",),
        ),
    ),
)
