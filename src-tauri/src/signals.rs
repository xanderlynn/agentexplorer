//! Deterministic signals derived from a session's prompts and shape: what
//! kind of task it was, how often the human had to correct the agent, and a
//! friction score modelled on requirements / steering / scope.
//!
//! These are heuristics. The UI labels them as such.

use serde::Serialize;

/// Ordered: the first category whose keywords match wins, so the more
/// specific intents come before the generic "feature".
const CATEGORIES: &[(&str, &[&str])] = &[
    ("review", &["review", "audit", "critique", "look over"]),
    (
        "bugfix",
        &[
            "fix",
            "bug",
            "error",
            "broken",
            "failing",
            "fails",
            "crash",
            "issue",
            "exception",
            "not working",
            "doesn't work",
        ],
    ),
    ("tests", &["test", "spec", "coverage", "e2e", "unit test"]),
    (
        "refactor",
        &[
            "refactor",
            "clean up",
            "cleanup",
            "rename",
            "restructure",
            "simplify",
            "reorganize",
            "extract",
        ],
    ),
    (
        "docs",
        &[
            "readme",
            "docs",
            "document",
            "documentation",
            "comment",
            "changelog",
        ],
    ),
    (
        "infra",
        &[
            "deploy",
            "pipeline",
            "ci ",
            "ci/",
            "github action",
            "workflow",
            "docker",
            "terraform",
            "kubernetes",
            "azure",
            "aws",
            "infra",
            "release",
        ],
    ),
    (
        "research",
        &[
            "explain",
            "investigate",
            "why ",
            "how does",
            "how do",
            "research",
            "compare",
            "what is",
            "what are",
            "understand",
            "review the",
            "options",
        ],
    ),
    (
        "feature",
        &[
            "add",
            "implement",
            "create",
            "build",
            "new ",
            "support",
            "make ",
        ],
    ),
];

pub fn categorize(first_prompt: &str, title: &str) -> &'static str {
    let text = format!(" {} {} ", first_prompt, title).to_lowercase();
    CATEGORIES
        .iter()
        .find(|(_, words)| words.iter().any(|w| text.contains(w)))
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
    "wrong",
    "revert",
    "undo",
    "you broke",
    "broke ",
    "why did you",
    "i said",
    "i asked",
    "try again",
    "again,",
];

/// A follow-up prompt that corrects the agent rather than moving on.
pub fn is_correction(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    p.starts_with("no ")
        || p.starts_with("no,")
        || p.starts_with("no.")
        || p == "no"
        || p.starts_with("stop")
        || CORRECTION_MARKERS.iter().any(|m| p.contains(m))
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
