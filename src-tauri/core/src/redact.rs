//! Strips secrets from the text Tilly keeps. A prompt you typed or an agent's
//! reply can hold a pasted API key or password; Tilly stores prompts, titles
//! and last replies, and a retro sends excerpts to Claude. Well-known token
//! shapes, private key blocks, JWTs, bearer tokens and `password=…`-style
//! values become `[redacted]` before any of that.

use std::borrow::Cow;

use anyhow::Result;
use rusqlite::{Connection, params};

const MARK: &str = "[redacted]";
const KEY_MARK: &str = "[redacted private key]";

/// Prefixes of well-known tokens; the prefix is kept, the rest redacted.
const PREFIXES: &[&str] = &[
    "github_pat_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "glpat-",
    "sk-ant-",
    "sk-proj-",
    "sk-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxs-",
    "npm_",
    "pypi-",
    "AIza",
    "AKIA",
    "ASIA",
];

/// Names whose `=` or `:` value is a secret.
const KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "client_secret",
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "secret_key",
    "accountkey",
    "token",
    "access_token",
    "refresh_token",
    "auth_token",
    "private_key",
];

type Span = (usize, usize, &'static str);

/// `text` with its secrets replaced; borrowed when there were none.
pub fn redact(text: &str) -> Cow<'_, str> {
    let mut spans = vec![];
    private_keys(text, &mut spans);
    tokens(text, &mut spans);
    assignments(text, &mut spans);
    if spans.is_empty() {
        return Cow::Borrowed(text);
    }
    spans.sort_by_key(|s| s.0);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (s, e, mark) in spans {
        if s < at {
            // Inside one already redacted.
            at = at.max(e);
            continue;
        }
        out.push_str(&text[at..s]);
        out.push_str(mark);
        at = e;
    }
    out.push_str(&text[at..]);
    Cow::Owned(out)
}

/// Replaces `s` with its redacted form, if it changed.
pub fn in_place(s: &mut String) {
    if let Cow::Owned(r) = redact(s) {
        *s = r;
    }
}

fn private_keys(t: &str, out: &mut Vec<Span>) {
    let mut from = 0;
    while let Some(i) = t[from..].find("-----BEGIN ") {
        let s = from + i;
        let Some(h) = t[s..].find("PRIVATE KEY-----").map(|j| s + j + 16) else {
            return;
        };
        if t[s..h].contains('\n') {
            from = s + 11;
            continue;
        }
        // A key cut off by truncation is redacted to the end.
        let e = t[h..]
            .find("-----END ")
            .and_then(|j| {
                let k = h + j + 9;
                t[k..].find("-----").map(|m| k + m + 5)
            })
            .unwrap_or(t.len());
        out.push((s, e, KEY_MARK));
        from = e;
    }
}

fn is_jwt(w: &str) -> bool {
    w.len() >= 40 && w.starts_with("eyJ") && {
        let parts: Vec<&str> = w.split('.').collect();
        parts.len() == 3 && parts.iter().all(|p| !p.is_empty())
    }
}

fn tokens(t: &str, out: &mut Vec<Span>) {
    let b = t.as_bytes();
    let is_tok = |c: u8| c.is_ascii_alphanumeric() || b"_-.".contains(&c);
    let (mut i, mut prev) = (0, (0, 0));
    while i < b.len() {
        if !is_tok(b[i]) {
            i += 1;
            continue;
        }
        let s = i;
        while i < b.len() && is_tok(b[i]) {
            i += 1;
        }
        let mut e = i;
        while e > s && b[e - 1] == b'.' {
            e -= 1;
        }
        let w = &t[s..e];
        if let Some(p) = PREFIXES.iter().find(|p| w.starts_with(**p)) {
            let rest = &w[p.len()..];
            if rest.len() >= 16 && rest.bytes().any(|c| c.is_ascii_digit()) {
                out.push((s + p.len(), e, MARK));
            }
        } else if is_jwt(w)
            || (w.len() >= 20
                && s == prev.1 + 1
                && b[prev.1] == b' '
                && matches!(&t[prev.0..prev.1], "Bearer" | "bearer"))
        {
            out.push((s, e, MARK));
        }
        prev = (s, e);
    }
}

fn assignments(t: &str, out: &mut Vec<Span>) {
    // ASCII lowercasing keeps byte offsets.
    let lower = t.to_ascii_lowercase();
    let b = t.as_bytes();
    for key in KEYS {
        let mut from = 0;
        while let Some(j) = lower[from..].find(key) {
            let k = from + j;
            from = k + key.len();
            // "MY_API_KEY=" counts; "mytoken=" doesn't.
            if k > 0 && b[k - 1].is_ascii_alphanumeric() {
                continue;
            }
            let mut i = k + key.len();
            if i < b.len() && matches!(b[i], b'"' | b'\'') {
                i += 1;
            }
            while i < b.len() && b[i] == b' ' {
                i += 1;
            }
            if i >= b.len() || !matches!(b[i], b'=' | b':') {
                continue;
            }
            i += 1;
            while i < b.len() && matches!(b[i], b' ' | b'"' | b'\'') {
                i += 1;
            }
            let s = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && !b"\"'`,;)}]".contains(&b[i]) {
                i += 1;
            }
            let v = &t[s..i];
            // "password: required" is prose; "password: hunter22" isn't.
            if v.len() >= 8 && !v.bytes().all(|c| c.is_ascii_lowercase()) {
                out.push((s, i, MARK));
            }
        }
    }
}

/// Bump when the patterns change, so stored text is scrubbed again.
const VERSION: i64 = 1;
const VERSION_KEY: &str = "redact.version";

/// Scrubs text stored before redaction existed (or before the patterns last
/// changed), including sessions whose logs are gone. Once per `VERSION`.
pub fn scrub_stored(conn: &Connection) -> Result<usize> {
    let done = crate::db::get_setting(conn, VERSION_KEY)?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    if done >= VERSION {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for (table, key, cols) in [
        ("prompts", "rowid", &["text"][..]),
        (
            "sessions",
            "id",
            &["title", "first_prompt", "last_message"][..],
        ),
    ] {
        for col in cols {
            let rows: Vec<(rusqlite::types::Value, String)> = tx
                .prepare(&format!(
                    "SELECT {key}, {col} FROM {table} WHERE {col} IS NOT NULL"
                ))?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut up = tx.prepare(&format!("UPDATE {table} SET {col} = ?1 WHERE {key} = ?2"))?;
            for (id, text) in rows {
                if let Cow::Owned(r) = redact(&text) {
                    up.execute(params![r, id])?;
                    n += 1;
                }
            }
        }
    }
    crate::db::set_setting(&tx, VERSION_KEY, &VERSION.to_string())?;
    tx.commit()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_secret_shapes() {
        let cases = [
            (
                "use ghp_abcdefghijklmnop1234567890 for CI",
                "use ghp_[redacted] for CI",
            ),
            (
                "export ANTHROPIC_API_KEY=sk-ant-api03-AbCdEf1234567890xyz",
                "export ANTHROPIC_API_KEY=[redacted]",
            ),
            ("aws AKIAIOSFODNN7EXAMPLE1 ok", "aws AKIA[redacted] ok"),
            (
                "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCY",
                "AWS_SECRET_ACCESS_KEY=[redacted]",
            ),
            (
                "pass it sk-ant-api03-AbCdEf1234567890xyz please",
                "pass it sk-ant-[redacted] please",
            ),
            (
                "curl -H 'Authorization: Bearer abcdefghij0123456789xyz'",
                "curl -H 'Authorization: Bearer [redacted]'",
            ),
            (
                "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N",
                "jwt [redacted]",
            ),
            ("db password=hunter2pass!", "db password=[redacted]"),
            (
                r#"{"client_secret": "Zx81-aa_9Qp"}"#,
                r#"{"client_secret": "[redacted]"}"#,
            ),
            (
                "DefaultEndpointsProtocol=https;AccountKey=abc123DEF456ghi789==;EndpointSuffix=core",
                "DefaultEndpointsProtocol=https;AccountKey=[redacted];EndpointSuffix=core",
            ),
            (
                "key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----\ndone",
                "key:\n[redacted private key]\ndone",
            ),
            (
                "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNz",
                "[redacted private key]",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(redact(input), want, "{input}");
        }
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for t in [
            "Fix the password reset flow and the token refresh",
            "password: required, token: optional",
            "set max_tokens: 16000 and the sk-learn model",
            "Refactor the secrets loader in src/auth/token.rs",
            "the ghp_ prefix marks a GitHub token",
            "Bearer tokens expire",
            "naïve café: ok",
        ] {
            assert!(matches!(redact(t), Cow::Borrowed(_)), "{t}");
        }
    }

    #[test]
    fn scrubs_stored_text_once() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, first_prompt, last_message, ingested_at)
             VALUES ('a', 'claude_code', 'a', '/f', 'token=abc123XYZ789', 'ok', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO prompts (session_id, idx, ts, text) VALUES ('a', 0, 0, 'my key ghp_abcdefghijklmnop1234567890')",
            [],
        )
        .unwrap();
        assert_eq!(scrub_stored(&conn).unwrap(), 2);
        let (fp, text): (String, String) = conn
            .query_row(
                "SELECT first_prompt, (SELECT text FROM prompts) FROM sessions",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (fp.as_str(), text.as_str()),
            ("token=[redacted]", "my key ghp_[redacted]")
        );
        assert_eq!(scrub_stored(&conn).unwrap(), 0, "only once");
    }
}
