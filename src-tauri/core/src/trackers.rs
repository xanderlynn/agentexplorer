//! Epics and features from issue trackers, to build a workstream from: a
//! GitHub issue with its sub-issues (or its task list), or an Azure DevOps
//! work item with its children. Fetched with your own `gh` or `az` sign-in,
//! so Tilly never stores a token.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::tasks::TaskStatus;
use crate::util::quiet_command;

/// The Azure DevOps resource id, for `az rest` to get a token for.
const ADO_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
/// Child items fetched per epic.
const MAX_ITEMS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    GitHub {
        owner: String,
        repo: String,
        number: u64,
    },
    Ado {
        /// "https://dev.azure.com/<org>"
        org: String,
        project: String,
        id: u64,
    },
}

/// One item under the epic.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Stable within the epic: the issue number or work item id, or the
    /// task list line.
    pub key: String,
    pub title: String,
    pub status: TaskStatus,
    pub url: Option<String>,
    /// Labels or tags, lowercased; "agent" and "human" set who does it.
    pub labels: Vec<String>,
    pub assignee: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Epic {
    pub title: String,
    pub url: String,
    pub status: TaskStatus,
    pub items: Vec<Item>,
    /// Text that marks a session as working on it, such as its URL or
    /// "owner/repo#12", for it and each item.
    pub needles: Vec<String>,
    /// The issue numbers, matched as "#12" in sessions in the same repository.
    pub numbers: Vec<u64>,
    /// The repository's name, for those matches.
    pub repo: Option<String>,
}

/// Reads a GitHub issue URL ("https://github.com/o/r/issues/12" or
/// "o/r#12") or an Azure DevOps work item URL.
pub fn parse(input: &str) -> Result<Ref> {
    let s = input.trim().trim_end_matches('/');
    let bad = || anyhow!("Paste a GitHub issue or Azure DevOps work item link");
    if let Some((repo, n)) = s.split_once('#').filter(|_| !s.contains("://")) {
        let (owner, repo) = repo.split_once('/').ok_or_else(bad)?;
        return Ok(Ref::GitHub {
            owner: owner.into(),
            repo: repo.into(),
            number: n.parse().map_err(|_| bad())?,
        });
    }
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .ok_or_else(bad)?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let parts: Vec<&str> = rest.split('/').collect();
    let id = |p: &str| p.parse::<u64>().map_err(|_| bad());
    match parts.as_slice() {
        ["github.com", owner, repo, "issues" | "pull", n, ..] => Ok(Ref::GitHub {
            owner: (*owner).into(),
            repo: (*repo).into(),
            number: id(n)?,
        }),
        ["dev.azure.com", org, project, "_workitems", "edit", n, ..] => Ok(Ref::Ado {
            org: format!("https://dev.azure.com/{org}"),
            project: decode(project),
            id: id(n)?,
        }),
        [host, project, "_workitems", "edit", n, ..] if host.ends_with(".visualstudio.com") => {
            Ok(Ref::Ado {
                org: format!("https://{host}"),
                project: decode(project),
                id: id(n)?,
            })
        }
        _ => Err(bad()),
    }
}

/// "My%20Project" → "My Project".
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%')
            .then(|| b.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        if let Some(h) = hex {
            out.push(h);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Fetches the epic and its items.
pub fn fetch(r: &Ref) -> Result<Epic> {
    match r {
        Ref::GitHub {
            owner,
            repo,
            number,
        } => {
            let base = format!("repos/{owner}/{repo}/issues/{number}");
            let issue = run(&gh()?, &["api", &base])?;
            let subs = run(&gh()?, &["api", &format!("{base}/sub_issues?per_page=100")])
                .unwrap_or(Value::Array(vec![]));
            Ok(github_epic(owner, repo, &issue, &subs))
        }
        Ref::Ado { org, project, id } => {
            let api = format!("{org}/{project}/_apis/wit/workitems");
            let parent = az_get(&format!("{api}/{id}?$expand=relations&api-version=7.1"))?;
            let ids = ado_children(&parent);
            let children = if ids.is_empty() {
                Value::Null
            } else {
                let list: Vec<String> = ids.iter().take(MAX_ITEMS).map(u64::to_string).collect();
                // The batch endpoint takes up to 200 ids.
                az_get(&format!(
                    "{api}?ids={}&fields=System.Title,System.State,System.Tags,System.AssignedTo&api-version=7.1",
                    list.join(",")
                ))?
            };
            Ok(ado_epic(org, project, &parent, &children))
        }
    }
}

fn gh() -> Result<PathBuf> {
    crate::util::find_program("gh", &[r"C:\Program Files\GitHub CLI\gh.exe"]).ok_or_else(|| {
        anyhow!("Install the GitHub CLI (gh) and run `gh auth login` to import GitHub issues")
    })
}

fn az_get(uri: &str) -> Result<Value> {
    let az = crate::util::find_program(
        "az",
        &[r"C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin\az.cmd"],
    )
    .ok_or_else(|| {
        anyhow!("Install the Azure CLI (az) and run `az login` to import Azure DevOps work items")
    })?;
    run(
        &az,
        &[
            "rest",
            "--resource",
            ADO_RESOURCE,
            "--uri",
            uri,
            "-o",
            "json",
        ],
    )
}

fn run(bin: &PathBuf, args: &[&str]) -> Result<Value> {
    let out = quiet_command(bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", bin.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("failed");
        bail!(
            "{}: {}",
            bin.file_stem().unwrap_or_default().to_string_lossy(),
            first.trim()
        );
    }
    serde_json::from_slice(&out.stdout).context("reading the tracker's reply")
}

fn labels(v: &Value) -> Vec<String> {
    v["labels"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|l| l["name"].as_str())
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default()
}

fn github_status(v: &Value) -> TaskStatus {
    if v["state"] == "closed" {
        if v["state_reason"] == "not_planned" {
            TaskStatus::Cancelled
        } else {
            TaskStatus::Done
        }
    } else if labels(v)
        .iter()
        .any(|l| l == "in progress" || l == "in-progress" || l == "doing")
    {
        TaskStatus::InProgress
    } else {
        TaskStatus::Pending
    }
}

fn github_epic(owner: &str, repo: &str, issue: &Value, subs: &Value) -> Epic {
    let number = issue["number"].as_u64().unwrap_or(0);
    let mut items: Vec<Item> = subs
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_ITEMS)
        .map(|s| Item {
            key: s["number"].as_u64().unwrap_or(0).to_string(),
            title: s["title"].as_str().unwrap_or("").to_string(),
            status: github_status(s),
            url: s["html_url"].as_str().map(String::from),
            labels: labels(s),
            assignee: s["assignees"][0]["login"].as_str().map(String::from),
        })
        .collect();
    // No sub-issues: use the task list in its description.
    if items.is_empty() {
        items = task_list(issue["body"].as_str().unwrap_or(""), owner, repo);
    }
    let mut numbers = vec![number];
    numbers.extend(items.iter().filter_map(|i| i.key.parse::<u64>().ok()));
    let needles = numbers
        .iter()
        .flat_map(|n| {
            [
                format!("github.com/{owner}/{repo}/issues/{n}"),
                format!("{owner}/{repo}#{n}"),
            ]
        })
        .collect();
    Epic {
        title: issue["title"]
            .as_str()
            .unwrap_or("Untitled issue")
            .to_string(),
        url: issue["html_url"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| format!("https://github.com/{owner}/{repo}/issues/{number}")),
        status: github_status(issue),
        items,
        needles,
        numbers,
        repo: Some(repo.to_string()),
    }
}

/// "- [x] Ship it" and "- [ ] #12" lines from a Markdown description.
fn task_list(body: &str, owner: &str, repo: &str) -> Vec<Item> {
    body.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let rest = l.strip_prefix("- [").or_else(|| l.strip_prefix("* ["))?;
            let (mark, text) = rest.split_once("] ")?;
            let done = mark.eq_ignore_ascii_case("x");
            if !done && mark != " " {
                return None;
            }
            let text = text.trim();
            let issue = text
                .strip_prefix('#')
                .and_then(|n| n.split_whitespace().next())
                .and_then(|n| n.parse::<u64>().ok());
            Some(Item {
                key: issue
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| text.to_string()),
                title: text.to_string(),
                status: if done {
                    TaskStatus::Done
                } else {
                    TaskStatus::Pending
                },
                url: issue.map(|n| format!("https://github.com/{owner}/{repo}/issues/{n}")),
                labels: vec![],
                assignee: None,
            })
        })
        .take(MAX_ITEMS)
        .collect()
}

fn ado_status(state: &str) -> TaskStatus {
    match state.to_lowercase().as_str() {
        "closed" | "done" | "resolved" | "completed" => TaskStatus::Done,
        "removed" | "cut" => TaskStatus::Cancelled,
        "active" | "in progress" | "doing" | "committed" | "in review" => TaskStatus::InProgress,
        _ => TaskStatus::Pending,
    }
}

/// The ids of a work item's children.
fn ado_children(parent: &Value) -> Vec<u64> {
    parent["relations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["rel"] == "System.LinkTypes.Hierarchy-Forward")
        .filter_map(|r| r["url"].as_str()?.rsplit('/').next()?.parse().ok())
        .collect()
}

fn ado_epic(org: &str, project: &str, parent: &Value, children: &Value) -> Epic {
    let id = parent["id"].as_u64().unwrap_or(0);
    let edit = |id: u64| format!("{org}/{}/_workitems/edit/{id}", project.replace(' ', "%20"));
    let items: Vec<Item> = children["value"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|w| {
            let f = &w["fields"];
            let n = w["id"].as_u64().unwrap_or(0);
            Item {
                key: n.to_string(),
                title: f["System.Title"].as_str().unwrap_or("").to_string(),
                status: ado_status(f["System.State"].as_str().unwrap_or("")),
                url: Some(edit(n)),
                labels: f["System.Tags"]
                    .as_str()
                    .unwrap_or("")
                    .split(';')
                    .map(|t| t.trim().to_lowercase())
                    .filter(|t| !t.is_empty())
                    .collect(),
                assignee: f["System.AssignedTo"]["displayName"]
                    .as_str()
                    .map(String::from),
            }
        })
        .collect();
    let mut needles = vec![];
    for n in std::iter::once(id).chain(items.iter().filter_map(|i| i.key.parse().ok())) {
        needles.push(format!("_workitems/edit/{n}"));
        needles.push(format!("AB#{n}"));
    }
    Epic {
        title: parent["fields"]["System.Title"]
            .as_str()
            .unwrap_or("Untitled work item")
            .to_string(),
        url: edit(id),
        status: ado_status(parent["fields"]["System.State"].as_str().unwrap_or("")),
        items,
        needles,
        numbers: vec![],
        repo: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_links() {
        let gh = |o: &str, r: &str, n| Ref::GitHub {
            owner: o.into(),
            repo: r.into(),
            number: n,
        };
        assert_eq!(
            parse("https://github.com/xanderlynn/tilly/issues/27").unwrap(),
            gh("xanderlynn", "tilly", 27)
        );
        assert_eq!(
            parse(" xanderlynn/tilly#27 ").unwrap(),
            gh("xanderlynn", "tilly", 27)
        );
        assert_eq!(
            parse("https://dev.azure.com/contoso/My%20App/_workitems/edit/42?src=x").unwrap(),
            Ref::Ado {
                org: "https://dev.azure.com/contoso".into(),
                project: "My App".into(),
                id: 42
            }
        );
        assert_eq!(
            parse("https://contoso.visualstudio.com/Web/_workitems/edit/7").unwrap(),
            Ref::Ado {
                org: "https://contoso.visualstudio.com".into(),
                project: "Web".into(),
                id: 7
            }
        );
        assert!(parse("https://example.com/x").is_err());
        assert!(parse("tilly#abc").is_err());
    }

    #[test]
    fn reads_github_epics() {
        let issue = json!({"number": 27, "title": "Workstream explorer", "state": "open",
            "html_url": "https://github.com/o/r/issues/27", "body": "- [x] ignored"});
        let subs = json!([
            {"number": 28, "title": "Capture tasks", "state": "closed", "state_reason": "completed",
             "html_url": "https://github.com/o/r/issues/28", "labels": [{"name": "agent"}], "assignees": []},
            {"number": 34, "title": "Review the design", "state": "open",
             "html_url": "https://github.com/o/r/issues/34", "labels": [{"name": "Human"}],
             "assignees": [{"login": "me"}]},
            {"number": 35, "title": "Dropped", "state": "closed", "state_reason": "not_planned"}
        ]);
        let e = github_epic("o", "r", &issue, &subs);
        assert_eq!(e.title, "Workstream explorer");
        assert_eq!(
            e.items
                .iter()
                .map(|i| (i.key.as_str(), i.status))
                .collect::<Vec<_>>(),
            [
                ("28", TaskStatus::Done),
                ("34", TaskStatus::Pending),
                ("35", TaskStatus::Cancelled)
            ]
        );
        assert_eq!(e.items[1].labels, ["human"]);
        assert_eq!(e.items[1].assignee.as_deref(), Some("me"));
        assert_eq!(e.numbers, [27, 28, 34, 35]);
        assert!(e.needles.contains(&"o/r#34".to_string()));

        // Without sub-issues, the description's task list.
        let issue = json!({"number": 1, "title": "Plan", "state": "open",
            "body": "Steps:\n- [x] Write spec\n- [ ] #5\n* [ ] Ship it\n- [?] no"});
        let e = github_epic("o", "r", &issue, &json!([]));
        assert_eq!(
            e.items
                .iter()
                .map(|i| (i.key.as_str(), i.status, i.url.is_some()))
                .collect::<Vec<_>>(),
            [
                ("Write spec", TaskStatus::Done, false),
                ("5", TaskStatus::Pending, true),
                ("Ship it", TaskStatus::Pending, false)
            ]
        );
    }

    #[test]
    fn reads_ado_epics() {
        let parent = json!({"id": 10, "fields": {"System.Title": "Checkout", "System.State": "Active"},
        "relations": [
            {"rel": "System.LinkTypes.Hierarchy-Forward", "url": "https://dev.azure.com/c/_apis/wit/workItems/11"},
            {"rel": "System.LinkTypes.Hierarchy-Reverse", "url": "https://dev.azure.com/c/_apis/wit/workItems/1"},
            {"rel": "System.LinkTypes.Hierarchy-Forward", "url": "https://dev.azure.com/c/_apis/wit/workItems/12"}
        ]});
        assert_eq!(ado_children(&parent), [11, 12]);
        let children = json!({"value": [
            {"id": 11, "fields": {"System.Title": "Swap SDK", "System.State": "Closed", "System.Tags": "agent; backend"}},
            {"id": 12, "fields": {"System.Title": "Approve rollout", "System.State": "New",
                "System.AssignedTo": {"displayName": "Sam"}}}
        ]});
        let e = ado_epic("https://dev.azure.com/c", "My App", &parent, &children);
        assert_eq!(e.title, "Checkout");
        assert_eq!(e.status, TaskStatus::InProgress);
        assert_eq!(e.url, "https://dev.azure.com/c/My%20App/_workitems/edit/10");
        assert_eq!(e.items[0].labels, ["agent", "backend"]);
        assert_eq!(e.items[0].status, TaskStatus::Done);
        assert_eq!(e.items[1].assignee.as_deref(), Some("Sam"));
        assert!(e.needles.contains(&"AB#12".to_string()));
    }

    /// Needs `gh` signed in and the network: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_a_real_github_epic() {
        let e = fetch(&parse("xanderlynn/tilly#27").unwrap()).unwrap();
        println!("{e:#?}");
        assert!(e.items.len() >= 8);
    }
}
