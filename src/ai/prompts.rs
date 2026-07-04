//! Prompt templates for AI tasks. Prompts are pointers, not payloads: they
//! name a commit range and tell the session to inspect it through git. The
//! harness owns context assembly (ADR-0005). Read-only is enforced by spawn
//! flags in `cli.rs`; the prompt text restates it only as belt-and-braces.

use crate::ai::thread::{Anchor, DiffSide};

fn range_text(base_sha: Option<&str>, head_sha: &str) -> String {
    match base_sha {
        Some(base) => format!("{base}..{head_sha}"),
        None => head_sha.to_string(),
    }
}

const TARGETING: &str = "Inspect the commits through git (`git show`, `git diff`, \
`git log`), not the working tree — the checkout may differ from the commits under \
review. Do not modify the repository in any way.";

pub fn summary_prompt(base_sha: Option<&str>, head_sha: &str) -> String {
    let range = range_text(base_sha, head_sha);
    format!(
        "Summarize the changes in {range} of this git repository for a code \
reviewer who has not read them yet: what changed, why it appears to have \
changed, and anything surprising. Respond in concise markdown. {TARGETING}"
    )
}

pub fn ask_prompt(anchor: &Anchor, selected_text: &str, question: &str) -> String {
    let side = match anchor.side {
        DiffSide::Old => "old side (before the change)",
        DiffSide::New => "new side (after the change)",
    };
    format!(
        "In this git repository, a reviewer is looking at commit {sha}, file \
`{file}`, lines {start}-{end} on the {side} of the diff. The selected text is:\n\n\
```\n{selected_text}\n```\n\nTheir question: {question}\n\nAnswer for that exact \
context, consulting the repository history as needed. {TARGETING}",
        sha = anchor.changeset_sha,
        file = anchor.file.display(),
        start = anchor.line_range.start(),
        end = anchor.line_range.end(),
    )
}

pub fn review_prompt(base_sha: Option<&str>, head_sha: &str) -> String {
    let range = range_text(base_sha, head_sha);
    format!(
        "Perform a thorough code review of the changes in {range} of this git \
repository. Look for correctness bugs, edge cases, and consequential design \
problems; do not report style nits. Your final response must be only the JSON \
findings document matching the provided schema — findings may be empty if the \
changes are sound. {TARGETING}"
    )
}

/// Schema handed to `--json-schema` for review turns; the CLI validates the
/// final result against it, so parsing failures surface as turn errors
/// instead of silent garbage.
pub const FINDINGS_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "findings": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "file": { "type": "string" },
          "line_start": { "type": "integer" },
          "line_end": { "type": "integer" },
          "severity": { "type": "string", "enum": ["high", "medium", "low"] },
          "title": { "type": "string" },
          "explanation": { "type": "string" }
        },
        "required": ["file", "line_start", "line_end", "severity", "title", "explanation"]
      }
    }
  },
  "required": ["findings"]
}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::thread::{Anchor, DiffSide};

    fn anchor() -> Anchor {
        Anchor {
            file: "src/graph/mod.rs".into(),
            line_range: 40..=52,
            side: DiffSide::New,
            changeset_sha: "def456".to_string(),
        }
    }

    #[test]
    fn summary_prompt_names_the_range_and_git() {
        let prompt = summary_prompt(Some("abc123"), "def456");
        assert!(prompt.contains("abc123..def456"));
        assert!(prompt.contains("git"));
        assert!(prompt.contains("not the working tree"));
    }

    #[test]
    fn summary_prompt_handles_root_commits() {
        // A root commit has no base; the prompt names the single commit.
        let prompt = summary_prompt(None, "def456");
        assert!(prompt.contains("def456"));
        assert!(!prompt.contains(".."));
    }

    #[test]
    fn ask_prompt_carries_anchor_selection_and_question() {
        let prompt = ask_prompt(&anchor(), "let x = y?;", "why the question mark?");
        assert!(prompt.contains("src/graph/mod.rs"));
        assert!(prompt.contains("40-52"));
        assert!(prompt.contains("def456"));
        assert!(prompt.contains("let x = y?;"));
        assert!(prompt.contains("why the question mark?"));
        assert!(prompt.contains("new side"));
    }

    #[test]
    fn review_prompt_demands_schema_conformant_findings() {
        let prompt = review_prompt(Some("abc123"), "def456");
        assert!(prompt.contains("abc123..def456"));
        assert!(prompt.contains("findings"));
        assert!(prompt.contains("not the working tree"));
    }

    #[test]
    fn findings_schema_is_valid_json_with_required_fields() {
        let schema: serde_json::Value =
            serde_json::from_str(FINDINGS_SCHEMA).expect("schema parses");
        let required = schema
            .pointer("/properties/findings/items/required")
            .and_then(serde_json::Value::as_array)
            .expect("findings items have required fields");
        let required: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        for field in [
            "file",
            "line_start",
            "line_end",
            "severity",
            "title",
            "explanation",
        ] {
            assert!(required.contains(&field), "{field} must be required");
        }
    }
}
