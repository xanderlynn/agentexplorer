//! The UI's copy of the engine's vocabulary: tool ids, statuses, groups and
//! shared thresholds, written to `src/generated.ts` so the two cannot drift.
//! A test fails when the file is out of date; regenerate it with
//! `UPDATE_GENERATED=1 cargo test -p tilly-core generated_ts`.

use std::fmt::Write;

use crate::live::LiveStatus;
use crate::sources::Source;
use crate::stats::MIN_SAMPLE;
use crate::tasks::TaskStatus;
use crate::team::AgentLiveStatus;
use crate::util::ErrorKind;
use crate::workstreams::TaskKind;

fn union(ids: impl IntoIterator<Item = String>) -> String {
    ids.into_iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn id_of<T: serde::Serialize>(v: T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("serializes as a string")
}

pub fn typescript() -> String {
    let mut ts = String::from(
        "// Generated from the Rust engine by src-tauri/core/src/codegen.rs. Do not edit;\n\
         // run `UPDATE_GENERATED=1 cargo test -p tilly-core generated_ts` in src-tauri.\n\n",
    );
    let sources = Source::ALL;
    let _ = writeln!(
        ts,
        "export type Source = {};\n",
        union(sources.iter().map(|s| s.id().to_string()))
    );
    ts += "/** Every supported tool, in the order Settings lists them. */\n";
    ts += "export const SOURCES: { id: Source; label: string; shortLabel: string }[] = [\n";
    for s in sources {
        let _ = writeln!(
            ts,
            "  {{ id: \"{}\", label: \"{}\", shortLabel: \"{}\" }},",
            s.id(),
            s.label(),
            s.short_label()
        );
    }
    ts += "];\n\n";

    let statuses = LiveStatus::ALL;
    ts += "/** One status per session; see live.rs `status_for`. */\n";
    let _ = writeln!(
        ts,
        "export type LiveStatus = {};\n",
        union(statuses.iter().map(id_of))
    );
    ts += "/** The part of the control center a status belongs to. */\n";
    let mut groups: Vec<String> = statuses.iter().map(|s| id_of(s.group())).collect();
    groups.sort();
    groups.dedup();
    let _ = writeln!(ts, "export type SessionGroup = {};\n", union(groups));
    ts += "export const STATUS_GROUP: Record<LiveStatus, SessionGroup> = {\n";
    for s in statuses {
        let _ = writeln!(ts, "  {}: \"{}\",", id_of(s), id_of(s.group()));
    }
    ts += "};\n\n";

    let _ = writeln!(
        ts,
        "export type LiveAgentStatus = {};\n",
        union(AgentLiveStatus::ALL.iter().map(id_of))
    );
    let _ = writeln!(
        ts,
        "export type TaskStatus = {};\n",
        union(TaskStatus::ALL.iter().map(id_of))
    );
    ts += "/** Who a task is for; see workstreams.rs `classify`. */\n";
    let _ = writeln!(
        ts,
        "export type TaskKind = {};\n",
        union(TaskKind::ALL.iter().map(id_of))
    );
    ts += "/** How a command failed; see util.rs `ErrorKind`. */\n";
    let _ = writeln!(
        ts,
        "export type ErrorKind = {};\n",
        union(ErrorKind::ALL.iter().map(id_of))
    );
    ts += "/** Below this many samples a rate is noise: shown greyed out or not at all. */\n";
    let _ = writeln!(ts, "export const MIN_SAMPLE = {MIN_SAMPLE};");
    ts
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_ts_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/generated.ts");
        let want = super::typescript();
        if std::env::var_os("UPDATE_GENERATED").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            have.replace("\r\n", "\n") == want,
            "src/generated.ts is out of date: run `UPDATE_GENERATED=1 cargo test -p tilly-core generated_ts` in src-tauri"
        );
    }
}
