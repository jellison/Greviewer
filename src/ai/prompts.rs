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

pub fn guide_prompt(base_sha: Option<&str>, head_sha: &str) -> String {
    let range = range_text(base_sha, head_sha);
    format!(
        "Prepare a review guide for the changes in {range} of this git \
repository. Respond with JSON matching the provided schema:\n\
- `summary`: a brief orientation for a product owner describing what \
changed in user-visible behavior and business rules, and why. Be ruthless \
about brevity: at most two short paragraphs and never more than five \
sentences total — reviewers read this before the diff, not instead of it. \
The summary must not mention file names, directory paths, line numbers, or \
code identifiers such as function, type, or variable names.\n\
- `review_order`: only the critical files — the ones where the behavioral \
and business-logic changes actually live — ordered so a reviewer builds \
understanding fastest (foundations before their consumers). Omit mechanical \
fallout: lockfiles, generated output, formatting-only churn, renames without \
edits, and call sites touched only to track a changed signature. There is no \
fixed count: a huge changeset may have many critical files or only a few — \
let the change decide. List each chosen file exactly once, use its path \
exactly as `git diff --name-only` prints it for this range, and give each a \
one-sentence `note` on its place in the reading order.\n\
{TARGETING}"
    )
}

/// Schema handed to `--json-schema` for guide turns; the CLI validates the
/// final result against it, so parsing failures surface as turn errors.
pub const GUIDE_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "summary": { "type": "string" },
    "review_order": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "path": { "type": "string" },
          "note": { "type": "string" }
        },
        "required": ["path", "note"]
      }
    }
  },
  "required": ["summary", "review_order"]
}"#;

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
    fn guide_prompt_names_the_range_and_forbids_code_identifiers() {
        let prompt = guide_prompt(Some("abc123"), "def456");
        assert!(prompt.contains("abc123..def456"));
        assert!(prompt.contains("product owner"));
        assert!(prompt.contains("never more than five sentences"));
        assert!(prompt.contains("must not mention file names"));
        assert!(prompt.contains("only the critical files"));
        assert!(prompt.contains("Omit mechanical"));
        assert!(prompt.contains("review_order"));
        assert!(prompt.contains("not the working tree"));
    }

    #[test]
    fn guide_prompt_handles_root_commits() {
        let prompt = guide_prompt(None, "def456");
        assert!(prompt.contains("def456"));
        assert!(!prompt.contains(".."));
    }

    #[test]
    fn guide_schema_is_valid_json_requiring_summary_and_order() {
        let schema: serde_json::Value = serde_json::from_str(GUIDE_SCHEMA).expect("schema parses");
        let required: Vec<&str> = schema
            .pointer("/required")
            .and_then(serde_json::Value::as_array)
            .expect("top-level required")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"summary"));
        assert!(required.contains(&"review_order"));
        let entry_required: Vec<&str> = schema
            .pointer("/properties/review_order/items/required")
            .and_then(serde_json::Value::as_array)
            .expect("entry required")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(entry_required.contains(&"path"));
        assert!(entry_required.contains(&"note"));
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
