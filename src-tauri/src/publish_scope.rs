//! What goes out when a folder is published, as the person chose it: nothing unless chosen,
//! a chosen folder covers everything under it, and the nearest explicit choice wins.

use serde::{Deserialize, Serialize};

/// One explicit choice. `path` is `/`-separated; `""` is the whole folder it is resolved against.
#[allow(dead_code)] // consumed by the publish wiring in later steps
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub path: String,
    pub include: bool,
}

#[allow(dead_code)] // consumed by the publish wiring in later steps
fn covers(choice: &str, rel: &str) -> bool {
    choice.is_empty()
        || rel.eq_ignore_ascii_case(choice)
        || (rel.len() > choice.len()
            && rel.is_char_boundary(choice.len())
            && rel.as_bytes()[choice.len()] == b'/'
            && rel[..choice.len()].eq_ignore_ascii_case(choice))
}

/// Whether `rel` goes out under `choices`.
#[allow(dead_code)] // consumed by the publish wiring in later steps
pub fn is_included(rel: &str, choices: &[Choice]) -> bool {
    choices
        .iter()
        .filter(|c| covers(&c.path, rel))
        .max_by_key(|c| c.path.len())
        .is_some_and(|c| c.include)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Case {
        why: String,
        choices: Vec<Choice>,
        rel: String,
        included: bool,
    }

    #[test]
    fn the_rule_matches_the_shared_cases() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../src/lib/publishScope.cases.json")).unwrap();
        for c in cases {
            assert_eq!(is_included(&c.rel, &c.choices), c.included, "{}", c.why);
        }
    }
}
