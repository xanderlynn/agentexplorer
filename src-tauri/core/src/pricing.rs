//! Approximate Anthropic API list prices, used only when a tool does not record
//! its own cost. Copilot is a subscription, so for Copilot sessions this is an
//! API-equivalent figure for comparison, not money spent.

use crate::model::TokenUsage;

/// USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

/// Ordered most-specific first: "opus-5-5" must match before "opus-5".
const TABLE: &[(&str, Price)] = &[
    (
        "fable-5-1",
        Price {
            input: 10.0,
            output: 50.0,
            cache_read: 0.25,
        },
    ),
    (
        "mythos-5-1",
        Price {
            input: 10.0,
            output: 50.0,
            cache_read: 0.25,
        },
    ),
    (
        "fable-5",
        Price {
            input: 10.0,
            output: 50.0,
            cache_read: 1.0,
        },
    ),
    (
        "mythos-5",
        Price {
            input: 10.0,
            output: 50.0,
            cache_read: 1.0,
        },
    ),
    (
        "opus-5-5",
        Price {
            input: 4.0,
            output: 20.0,
            cache_read: 0.20,
        },
    ),
    (
        "opus-5",
        Price {
            input: 5.0,
            output: 25.0,
            cache_read: 0.50,
        },
    ),
    (
        "opus-4-8",
        Price {
            input: 5.0,
            output: 25.0,
            cache_read: 0.50,
        },
    ),
    (
        "opus-4-7",
        Price {
            input: 5.0,
            output: 25.0,
            cache_read: 0.50,
        },
    ),
    (
        "opus-4-6",
        Price {
            input: 5.0,
            output: 25.0,
            cache_read: 0.50,
        },
    ),
    (
        "opus-4-5",
        Price {
            input: 5.0,
            output: 25.0,
            cache_read: 0.50,
        },
    ),
    (
        "opus-4",
        Price {
            input: 15.0,
            output: 75.0,
            cache_read: 1.50,
        },
    ),
    (
        "sonnet-5-5",
        Price {
            input: 2.0,
            output: 10.0,
            cache_read: 0.20,
        },
    ),
    (
        "sonnet-5",
        Price {
            input: 2.0,
            output: 10.0,
            cache_read: 0.20,
        },
    ),
    (
        "sonnet-4",
        Price {
            input: 3.0,
            output: 15.0,
            cache_read: 0.30,
        },
    ),
    (
        "sonnet-3",
        Price {
            input: 3.0,
            output: 15.0,
            cache_read: 0.30,
        },
    ),
    (
        "haiku-5-5",
        Price {
            input: 0.10,
            output: 0.50,
            cache_read: 0.01,
        },
    ),
    (
        "haiku-4-5",
        Price {
            input: 1.0,
            output: 5.0,
            cache_read: 0.10,
        },
    ),
    (
        "haiku-3-5",
        Price {
            input: 0.80,
            output: 4.0,
            cache_read: 0.08,
        },
    ),
];

/// Cache writes (5-minute TTL) bill at 1.25x the input rate.
const CACHE_WRITE_MULTIPLIER: f64 = 1.25;

fn normalize(model: &str) -> String {
    model.to_ascii_lowercase().replace('.', "-")
}

pub fn price_for(model: &str) -> Option<Price> {
    let m = normalize(model);
    TABLE
        .iter()
        .find(|(key, _)| m.contains(key))
        .map(|(_, p)| *p)
}

pub fn estimate_cost(model: &str, usage: &TokenUsage) -> Option<f64> {
    let p = price_for(model)?;
    let per = |tokens: u64, rate: f64| tokens as f64 * rate / 1_000_000.0;
    Some(
        per(usage.input, p.input)
            + per(usage.output, p.output)
            + per(usage.cache_read, p.cache_read)
            + per(usage.cache_write, p.input * CACHE_WRITE_MULTIPLIER),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_most_specific_model_first() {
        assert_eq!(price_for("claude-opus-5-5").unwrap().input, 4.0);
        assert_eq!(price_for("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(price_for("claude-sonnet-4.6").unwrap().input, 3.0);
        assert_eq!(price_for("claude-fable-5-1").unwrap().cache_read, 0.25);
        assert!(price_for("gpt-5").is_none());
    }

    #[test]
    fn estimates_all_token_kinds() {
        let usage = TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_write: 1_000_000,
        };
        let cost = estimate_cost("claude-sonnet-5-5", &usage).unwrap();
        assert!((cost - (2.0 + 10.0 + 0.2 + 2.5)).abs() < 1e-9);
    }
}
