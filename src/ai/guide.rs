//! Parse and validate the guide JSON a guide turn returns
//! (schema: `prompts::GUIDE_SCHEMA`). Validation is defensive: the CLI
//! enforces the schema shape, but paths are checked against the changeset
//! here so a hallucinated path can never become a dead link in the UI.

use std::collections::HashSet;

use crate::reviews::{ReviewGuide, ReviewGuideEntry};

pub fn parse_guide(
    text: &str,
    changeset_paths: &HashSet<String>,
    generated_at: i64,
) -> Result<ReviewGuide, String> {
    #[derive(serde::Deserialize)]
    struct RawGuide {
        summary: String,
        #[serde(default)]
        review_order: Vec<RawEntry>,
    }
    #[derive(serde::Deserialize)]
    struct RawEntry {
        path: String,
        note: String,
    }

    let raw: RawGuide =
        serde_json::from_str(text).map_err(|err| format!("guide response did not parse: {err}"))?;
    let review_order = raw
        .review_order
        .into_iter()
        .filter(|entry| changeset_paths.contains(&entry.path))
        .map(|entry| ReviewGuideEntry {
            path: entry.path,
            note: entry.note,
        })
        .collect();
    Ok(ReviewGuide {
        summary: raw.summary,
        review_order,
        generated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> HashSet<String> {
        list.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn parses_a_well_formed_guide() {
        let text = r#"{"summary":"Sessions now expire.","review_order":
            [{"path":"src/a.rs","note":"read first"},{"path":"src/b.rs","note":"consumer"}]}"#;
        let guide = parse_guide(text, &paths(&["src/a.rs", "src/b.rs"]), 42).expect("parses");
        assert_eq!(guide.summary, "Sessions now expire.");
        assert_eq!(guide.review_order.len(), 2);
        assert_eq!(guide.review_order[0].path, "src/a.rs");
        assert_eq!(guide.generated_at, 42);
    }

    #[test]
    fn drops_entries_whose_path_is_not_in_the_changeset() {
        let text = r#"{"summary":"s","review_order":
            [{"path":"src/real.rs","note":"n"},{"path":"src/hallucinated.rs","note":"n"}]}"#;
        let guide = parse_guide(text, &paths(&["src/real.rs"]), 0).expect("parses");
        assert_eq!(guide.review_order.len(), 1);
        assert_eq!(guide.review_order[0].path, "src/real.rs");
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        let err = parse_guide("not json", &paths(&[]), 0).expect_err("errors");
        assert!(err.contains("did not parse"));
    }

    #[test]
    fn missing_review_order_defaults_to_empty() {
        let guide = parse_guide(r#"{"summary":"s"}"#, &paths(&[]), 0).expect("parses");
        assert!(guide.review_order.is_empty());
    }
}
