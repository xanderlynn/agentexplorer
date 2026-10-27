//! Deterministic signals derived from a session's prompts and shape: what
//! kind of task it was, how often the human had to correct the agent, and a
//! friction score modelled on requirements / steering / scope.
//!
//! These are heuristics. The UI labels them as such.

use serde::Serialize;

/// Lowercases and splits into words (letters, digits and apostrophes), so
/// keywords match whole words: "latest" does not contain the word "test".
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['\u{2019}', '\u{2018}'], "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// A keyword is a word or phrase matched on word boundaries. A trailing `*`
/// matches any word starting with the stem ("fail*" matches "failing").
fn matches(words: &[String], keyword: &str) -> bool {
    let parts: Vec<&str> = keyword.split(' ').collect();
    if parts.is_empty() || words.len() < parts.len() {
        return false;
    }
    words.windows(parts.len()).any(|window| {
        window
            .iter()
            .zip(&parts)
            .all(|(w, p)| match p.strip_suffix('*') {
                Some(stem) => w.starts_with(stem),
                None => w == p,
            })
    })
}

/// Ordered: the first category whose keywords match wins, so the more
/// specific intents come before the generic "feature".
const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "review",
        &["review", "reviewing", "audit*", "critique", "look over"],
    ),
    (
        "bugfix",
        &[
            "fix",
            "fixes",
            "fixing",
            "bug*",
            "error*",
            "broken",
            "fail*",
            "crash*",
            "issue*",
            "exception*",
            "not working",
            "doesn't work",
            "regression*",
        ],
    ),
    (
        "tests",
        &[
            "test",
            "tests",
            "testing",
            "unit test*",
            "spec",
            "specs",
            "coverage",
            "e2e",
        ],
    ),
    (
        "refactor",
        &[
            "refactor*",
            "clean up",
            "cleanup",
            "rename*",
            "restructur*",
            "simplif*",
            "reorganiz*",
            "extract*",
            "dedupe",
            "deduplicate",
        ],
    ),
    (
        "docs",
        &[
            "readme",
            "docs",
            "document*",
            "comments",
            "changelog",
            "jsdoc",
            "docstring*",
        ],
    ),
    (
        "infra",
        &[
            "deploy*",
            "pipeline*",
            "ci",
            "github action*",
            "workflow*",
            "docker*",
            "terraform",
            "kubernetes",
            "k8s",
            "azure",
            "aws",
            "gcp",
            "infra*",
            "release*",
        ],
    ),
    (
        "research",
        &[
            "explain*",
            "investigat*",
            "why",
            "how does",
            "how do",
            "research*",
            "compare*",
            "what is",
            "what are",
            "understand*",
            "options",
            "tradeoff*",
            "trade off*",
        ],
    ),
    (
        "feature",
        &[
            "add",
            "adding",
            "adds",
            "implement*",
            "create*",
            "build*",
            "new",
            "support*",
            "make",
            "introduce*",
        ],
    ),
];

pub fn categorize(first_prompt: &str, title: &str) -> &'static str {
    let w = words(&format!("{first_prompt} {title}"));
    CATEGORIES
        .iter()
        .find(|(_, keywords)| keywords.iter().any(|k| matches(&w, k)))
        .map(|(c, _)| *c)
        .unwrap_or("other")
}

/// Phrases that usually mean the previous turn missed.
const CORRECTION_MARKERS: &[&str] = &[
    "that's not",
    "thats not",
    "that is not",
    "not what i",
    "doesn't work",
    "does not work",
    "didn't work",
    "still not",
    "still broken",
    "still failing",
    "still doesn't",
    "still wrong",
    "that's wrong",
    "is wrong",
    "wrong again",
    "revert",
    "undo",
    "you broke",
    "it broke",
    "why did you",
    "i said",
    "i asked",
    "try again",
];

/// "No" that opens a reply but isn't a correction ("no worries, now…").
const BENIGN_NO: &[&str] = &["worries", "problem", "need", "rush", "thanks"];

/// A follow-up prompt that corrects the agent rather than moving on.
pub fn is_correction(prompt: &str) -> bool {
    let w = words(prompt);
    let opens_with_no = w.first().is_some_and(|f| f == "no" || f == "nope")
        && w.get(1)
            .is_none_or(|next| !BENIGN_NO.contains(&next.as_str()));
    let opens_with_stop = w.first().is_some_and(|f| f == "stop");
    opens_with_no || opens_with_stop || CORRECTION_MARKERS.iter().any(|m| matches(&w, m))
}

pub fn count_corrections<'a>(prompts: impl Iterator<Item = &'a str>) -> u64 {
    // The opening prompt can't correct anything.
    prompts.skip(1).filter(|p| is_correction(p)).count() as u64
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Friction {
    /// How clearly the opening prompt set the goal (5 = clear).
    pub requirements: u8,
    /// How little mid-course correction the session needed (5 = none).
    pub steering: u8,
    /// How well bounded the session was (5 = tight).
    pub scope: u8,
    pub overall: f64,
}

/// `prompt_len` is the opening prompt's full length; `first_prompt` may be a
/// truncated copy, used only to look for concrete anchors.
pub fn friction(
    first_prompt: &str,
    prompt_len: usize,
    corrections: i64,
    interruptions: i64,
    user_turns: i64,
    active_minutes: i64,
) -> Friction {
    let mut requirements: u8 = match prompt_len {
        0..40 => 1,
        40..100 => 2,
        100..250 => 3,
        250..600 => 4,
        _ => 5,
    };
    // Concrete anchors (paths, code, acceptance criteria) make a goal clearer.
    let lower = first_prompt.to_lowercase();
    let anchored = first_prompt.contains('`')
        || first_prompt.contains('/')
        || [
            "should",
            "must",
            "acceptance",
            "done when",
            "so that",
            "test",
        ]
        .iter()
        .any(|w| lower.contains(w));
    if anchored && requirements < 5 {
        requirements += 1;
    }

    let steering = 5u8.saturating_sub((corrections + interruptions).clamp(0, 4) as u8);

    let scope: u8 = match (active_minutes, user_turns) {
        (m, t) if m > 120 || t > 25 => 1,
        (m, t) if m > 60 || t > 15 => 2,
        (m, t) if m > 30 || t > 8 => 3,
        (m, t) if m > 15 || t > 4 => 4,
        _ => 5,
    };

    Friction {
        requirements,
        steering,
        scope,
        overall: (requirements + steering + scope) as f64 / 3.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorizes_by_first_matching_intent() {
        assert_eq!(categorize("Fix the failing login test", ""), "bugfix");
        assert_eq!(categorize("Add CSV export to reports", ""), "feature");
        assert_eq!(categorize("Refactor the billing service", ""), "refactor");
        assert_eq!(
            categorize("Explain how does auth work here", ""),
            "research"
        );
        assert_eq!(categorize("hello", ""), "other");
        // Whole words only.
        assert_eq!(categorize("Explain the latest changes", ""), "research");
        assert_eq!(categorize("Add a preview pane", ""), "feature");
        assert_eq!(
            categorize("Update the padding on the address form", ""),
            "other"
        );
    }

    #[test]
    fn detects_corrections_after_the_first_prompt() {
        let prompts = [
            "No, fix it",
            "No, that's not it",
            "looks good, now add docs",
            "still failing",
        ];
        assert_eq!(count_corrections(prompts.iter().copied()), 2);
        assert!(!is_correction("Now add pagination"));
        assert!(!is_correction("No worries, now add the docs"));
        assert!(!is_correction("Document the undocumented flags"));
        assert!(!is_correction("Use a stopwatch for timing"));
        assert!(is_correction("Nope, revert that"));
    }

    #[test]
    fn friction_rewards_clear_bounded_sessions() {
        let prompt = "Add a CSV export button to src/reports/page.tsx. It should include the visible columns and have a unit test.";
        let clear = friction(prompt, prompt.len(), 0, 0, 3, 12);
        assert_eq!((clear.requirements, clear.steering, clear.scope), (4, 5, 5));
        let rough = friction("fix it", 6, 3, 2, 30, 150);
        assert_eq!((rough.requirements, rough.steering, rough.scope), (1, 1, 1));
    }
}
