//! Epics and features from issue trackers, to build a workstream from: a
//! GitHub issue with its sub-issues (nested, or else the task list in its
//! description), or an Azure DevOps work item with its children, down to
//! `MAX_DEPTH` levels. Fetched with your own `gh` or `az` sign-in, so Tilly
//! never stores a token.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::tasks::TaskStatus;
use crate::util::quiet_command;

/// The Azure DevOps resource id, for `az rest` to get a token for.
const ADO_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
/// Items fetched per epic, across all levels.
const MAX_ITEMS: usize = 1000;
/// Levels below the epic: epic → feature → story → task.
const MAX_DEPTH: u8 = 3;
/// Work items per Azure DevOps batch request.
const ADO_BATCH: usize = 200;
/// A `gh` or `az` call taking longer than this is given up on.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

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
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Item {
    /// Stable within the epic: the issue number or work item id, or the
    /// task list line.
    pub key: String,
    pub title: String,
    pub status: TaskStatus,
    pub url: Option<String>,
    /// Labels or tags, lowercased; "agent" and "human" set who does it.
    pub labels: Vec<String>,
    /// Who it's assigned to, joined for display.
    pub assignee: Option<String>,
    /// Assigned to you (the `gh` or `az` signed-in user).
    pub mine: bool,
    /// The item it sits under; none for the epic's direct children.
    pub parent: Option<String>,
    /// 0 for the epic's direct children.
    pub depth: u8,
}

/// Text in a session that ties it to the epic or one of its items.
#[derive(Debug, Clone, PartialEq)]
pub struct Needle {
    pub text: String,
    /// The item it names; none for the epic itself.
    pub item: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Epic {
    pub title: String,
    pub url: String,
    pub status: TaskStatus,
    pub items: Vec<Item>,
    /// Links and references such as "owner/repo#12" or "AB#12".
    pub needles: Vec<Needle>,
    /// Issue numbers matched as "#12" in sessions in the same repository,
    /// and the item each names.
    pub numbers: Vec<(u64, Option<String>)>,
    /// "owner/repo", for those matches and to show where it lives.
    pub repo: Option<String>,
}

impl Epic {
    pub fn closed(&self) -> bool {
        matches!(self.status, TaskStatus::Done | TaskStatus::Cancelled)
    }
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

/// Fetches the epic and everything under it.
pub fn fetch(r: &Ref) -> Result<Epic> {
    match r {
        Ref::GitHub {
            owner,
            repo,
            number,
        } => {
            let gh = gh()?;
            let issue = run(
                &gh,
                &["api", &format!("repos/{owner}/{repo}/issues/{number}")],
            )?;
            let me = run(&gh, &["api", "user"])
                .ok()
                .and_then(|u| u["login"].as_str().map(str::to_lowercase));
            let mut items = vec![];
            if issue["sub_issues_summary"]["total"].as_u64().unwrap_or(0) > 0 {
                github_children(&gh, &issue, None, 0, me.as_deref(), &mut items)?;
            } else {
                // No sub-issues: use the task list in its description.
                items = task_list(issue["body"].as_str().unwrap_or(""), owner, repo);
            }
            Ok(github_epic(owner, repo, &issue, items))
        }
        Ref::Ado { org, project, id } => {
            let az = az()?;
            let api = format!("{org}/{project}/_apis/wit/workitems");
            let parent = az_get(
                &az,
                &format!("{api}/{id}?$expand=relations&api-version=7.1"),
            )?;
            let me = run(&az, &["account", "show"])
                .ok()
                .and_then(|a| a["user"]["name"].as_str().map(str::to_lowercase));
            let mut items = vec![];
            let mut level: Vec<(u64, Option<String>)> = ado_children(&parent)
                .into_iter()
                .map(|c| (c, None))
                .collect();
            for depth in 0..MAX_DEPTH {
                if level.is_empty() || items.len() >= MAX_ITEMS {
                    break;
                }
                let mut next = vec![];
                for chunk in level.chunks(ADO_BATCH) {
                    let ids: Vec<String> = chunk.iter().map(|(i, _)| i.to_string()).collect();
                    let batch = az_get(
                        &az,
                        &format!(
                            "{api}?ids={}&$expand=relations&api-version=7.1",
                            ids.join(",")
                        ),
                    )?;
                    for w in batch["value"].as_array().into_iter().flatten() {
                        let n = w["id"].as_u64().unwrap_or(0);
                        let parent_key = chunk
                            .iter()
                            .find(|(i, _)| *i == n)
                            .and_then(|(_, p)| p.clone());
                        let mut it = ado_item(org, project, w, me.as_deref());
                        it.parent = parent_key;
                        it.depth = depth;
                        next.extend(
                            ado_children(w)
                                .into_iter()
                                .map(|c| (c, Some(it.key.clone()))),
                        );
                        items.push(it);
                    }
                }
                level = next;
            }
            items.truncate(MAX_ITEMS);
            Ok(ado_epic(org, project, &parent, items))
        }
    }
}

/// Posts `body` (Markdown) as a comment on the epic or item at `r`, as you,
/// and returns the comment's link. Only on your say-so.
pub fn comment(r: &Ref, body: &str) -> Result<String> {
    match r {
        Ref::GitHub {
            owner,
            repo,
            number,
        } => {
            let out = run(
                &gh()?,
                &[
                    "api",
                    "-X",
                    "POST",
                    &format!("repos/{owner}/{repo}/issues/{number}/comments"),
                    "-f",
                    &format!("body={body}"),
                ],
            )?;
            Ok(out["html_url"].as_str().unwrap_or_default().to_string())
        }
        Ref::Ado { org, project, id } => {
            let az = az()?;
            let payload = serde_json::json!({ "text": markdown_to_html(body) }).to_string();
            run(
                &az,
                &[
                    "rest",
                    "--method",
                    "post",
                    "--resource",
                    ADO_RESOURCE,
                    "--uri",
                    &format!(
                        "{org}/{project}/_apis/wit/workItems/{id}/comments?api-version=7.1-preview.4"
                    ),
                    "--headers",
                    "Content-Type=application/json",
                    "--body",
                    &payload,
                    "-o",
                    "json",
                ],
            )?;
            Ok(ado_edit(org, project, *id))
        }
    }
}

/// Labels (GitHub) or tags (Azure DevOps) the item at `r` as `agent` or
/// `human` work, removing the other, so your choice reaches the tracker.
pub fn label_kind(r: &Ref, human: bool) -> Result<()> {
    let (add, drop) = if human {
        ("human", "agent")
    } else {
        ("agent", "human")
    };
    match r {
        Ref::GitHub {
            owner,
            repo,
            number,
        } => {
            let gh = gh()?;
            let base = format!("repos/{owner}/{repo}/issues/{number}/labels");
            run(
                &gh,
                &["api", "-X", "POST", &base, "-f", &format!("labels[]={add}")],
            )?;
            // Not labelled that way is fine.
            let _ = run(&gh, &["api", "-X", "DELETE", &format!("{base}/{drop}")]);
            Ok(())
        }
        Ref::Ado { org, project, id } => {
            let az = az()?;
            let api = format!("{org}/{project}/_apis/wit/workitems/{id}?api-version=7.1");
            let item = az_get(&az, &api)?;
            let mut tags: Vec<String> = item["fields"]["System.Tags"]
                .as_str()
                .unwrap_or("")
                .split(';')
                .map(|t| t.trim().to_string())
                .filter(|t| {
                    !t.is_empty() && !t.eq_ignore_ascii_case(drop) && !t.eq_ignore_ascii_case(add)
                })
                .collect();
            tags.push(add.into());
            let patch = serde_json::json!([{ "op": "add", "path": "/fields/System.Tags", "value": tags.join("; ") }]).to_string();
            run(
                &az,
                &[
                    "rest",
                    "--method",
                    "patch",
                    "--resource",
                    ADO_RESOURCE,
                    "--uri",
                    &api,
                    "--headers",
                    "Content-Type=application/json-patch+json",
                    "--body",
                    &patch,
                    "-o",
                    "json",
                ],
            )?;
            Ok(())
        }
    }
}

/// Just enough Markdown for an Azure DevOps comment, which takes HTML.
fn markdown_to_html(md: &str) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut out = String::new();
    let mut in_list = false;
    for line in md.lines() {
        let l = line.trim_end();
        if let Some(item) = l.strip_prefix("- ") {
            if !in_list {
                out.push_str("<ul>");
                in_list = true;
            }
            out.push_str(&format!("<li>{}</li>", esc(item)));
            continue;
        }
        if in_list {
            out.push_str("</ul>");
            in_list = false;
        }
        if let Some(h) = l.strip_prefix("### ") {
            out.push_str(&format!("<h3>{}</h3>", esc(h)));
        } else if !l.is_empty() {
            out.push_str(&format!("<p>{}</p>", esc(l)));
        }
    }
    if in_list {
        out.push_str("</ul>");
    }
    out
}

fn gh() -> Result<PathBuf> {
    crate::util::find_program("gh", &[r"C:\Program Files\GitHub CLI\gh.exe"]).ok_or_else(|| {
        anyhow!("Install the GitHub CLI (gh) and run `gh auth login` to import GitHub issues")
    })
}

fn az() -> Result<PathBuf> {
    crate::util::find_program(
        "az",
        &[r"C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin\az.cmd"],
    )
    .ok_or_else(|| {
        anyhow!("Install the Azure CLI (az) and run `az login` to import Azure DevOps work items")
    })
}

fn az_get(az: &PathBuf, uri: &str) -> Result<Value> {
    run(
        az,
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
    let mut cmd = quiet_command(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let tool = bin
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let out = crate::util::output_with_timeout(&mut cmd, CALL_TIMEOUT)
        .with_context(|| format!("running {}", bin.display()))?
        .ok_or_else(|| anyhow!("{tool} didn't answer within {}s", CALL_TIMEOUT.as_secs()))?;
    if !out.status.success() {
        bail!(failure(&tool, &String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).context("reading the tracker's reply")
}

/// A failed `gh` or `az` call in plain words.
fn failure(tool: &str, stderr: &str) -> String {
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("failed");
    let detail = first
        .strip_prefix(&format!("{tool}: "))
        .or_else(|| first.strip_prefix("ERROR: "))
        .unwrap_or(first);
    let lower = stderr.to_lowercase();
    if lower.contains("http 404") || lower.contains("does not exist") {
        "Not found: it may have been deleted or moved, or your sign-in can't see it".into()
    } else if lower.contains("auth login")
        || lower.contains("az login")
        || lower.contains("http 401")
    {
        format!(
            "Not signed in: run `{tool} {}`",
            if tool == "az" { "login" } else { "auth login" }
        )
    } else {
        format!("{tool}: {detail}")
    }
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
    } else if labels(v).iter().any(|l| l == "blocked") {
        TaskStatus::Blocked
    } else {
        TaskStatus::Pending
    }
}

/// "https://github.com/o/r/issues/12" → ("o/r", 12).
fn github_ref(url: &str) -> Option<(String, u64)> {
    let rest = url.strip_prefix("https://github.com/")?;
    let mut p = rest.split('/');
    let (owner, repo, kind, n) = (p.next()?, p.next()?, p.next()?, p.next()?);
    (kind == "issues").then(|| Some((format!("{owner}/{repo}"), n.parse().ok()?)))?
}

fn github_item(s: &Value, me: Option<&str>) -> Item {
    let assignees: Vec<String> = s["assignees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["login"].as_str().map(String::from))
        .collect();
    let url = s["html_url"].as_str().map(String::from);
    Item {
        // The number alone within the epic's repo; repo#number elsewhere.
        key: url
            .as_deref()
            .and_then(github_ref)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| s["number"].as_u64().unwrap_or(0).to_string()),
        title: s["title"].as_str().unwrap_or("").to_string(),
        status: github_status(s),
        url,
        labels: labels(s),
        mine: me.is_some_and(|me| assignees.iter().any(|a| a.eq_ignore_ascii_case(me))),
        assignee: (!assignees.is_empty()).then(|| assignees.join(", ")),
        parent: None,
        depth: 0,
    }
}

/// The sub-issues of `issue`, and theirs, depth first.
fn github_children(
    gh: &PathBuf,
    issue: &Value,
    parent: Option<&str>,
    depth: u8,
    me: Option<&str>,
    items: &mut Vec<Item>,
) -> Result<()> {
    let Some(api) = issue["url"]
        .as_str()
        .and_then(|u| u.strip_prefix("https://api.github.com/"))
    else {
        bail!("GitHub's reply had no issue address");
    };
    let pages = run(
        gh,
        &[
            "api",
            "--paginate",
            "--slurp",
            &format!("{api}/sub_issues?per_page=100"),
        ],
    )?;
    let subs: Vec<&Value> = pages
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p.as_array().into_iter().flatten())
        .collect();
    for s in subs {
        if items.len() >= MAX_ITEMS {
            break;
        }
        let mut it = github_item(s, me);
        it.parent = parent.map(String::from);
        it.depth = depth;
        let key = it.key.clone();
        items.push(it);
        if depth + 1 < MAX_DEPTH && s["sub_issues_summary"]["total"].as_u64().unwrap_or(0) > 0 {
            github_children(gh, s, Some(&key), depth + 1, me, items)?;
        }
    }
    Ok(())
}

fn github_epic(owner: &str, repo: &str, issue: &Value, items: Vec<Item>) -> Epic {
    let number = issue["number"].as_u64().unwrap_or(0);
    let url = issue["html_url"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| format!("https://github.com/{owner}/{repo}/issues/{number}"));
    // The issue may have moved: name it by where it is now.
    let (full, _) = github_ref(&url).unwrap_or_else(|| (format!("{owner}/{repo}"), number));
    let mut needles = vec![];
    let mut numbers = vec![(number, None)];
    let mut add = |link: &str, item: Option<String>| {
        if let Some((r, n)) = github_ref(link) {
            needles.push(Needle {
                text: format!("github.com/{r}/issues/{n}"),
                item: item.clone(),
            });
            needles.push(Needle {
                text: format!("{r}#{n}"),
                item: item.clone(),
            });
            if r.eq_ignore_ascii_case(&full) && item.is_some() {
                numbers.push((n, item));
            }
        }
    };
    add(&url, None);
    for it in &items {
        if let Some(u) = &it.url {
            add(u, Some(it.key.clone()));
        }
    }
    Epic {
        title: issue["title"]
            .as_str()
            .unwrap_or("Untitled issue")
            .to_string(),
        url,
        status: github_status(issue),
        items,
        needles,
        numbers,
        repo: Some(full),
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
                // A plain line is keyed by its words, so a reworded line is a new item.
                key: issue.map(|n| n.to_string()).unwrap_or_else(|| {
                    text.split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase()
                }),
                title: text.to_string(),
                status: if done {
                    TaskStatus::Done
                } else {
                    TaskStatus::Pending
                },
                url: issue.map(|n| format!("https://github.com/{owner}/{repo}/issues/{n}")),
                ..Default::default()
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
        "blocked" | "on hold" => TaskStatus::Blocked,
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

fn ado_edit(org: &str, project: &str, id: u64) -> String {
    format!("{org}/{}/_workitems/edit/{id}", project.replace(' ', "%20"))
}

fn ado_item(org: &str, project: &str, w: &Value, me: Option<&str>) -> Item {
    let f = &w["fields"];
    let n = w["id"].as_u64().unwrap_or(0);
    let who = &f["System.AssignedTo"];
    Item {
        key: n.to_string(),
        title: f["System.Title"].as_str().unwrap_or("").to_string(),
        status: ado_status(f["System.State"].as_str().unwrap_or("")),
        url: Some(ado_edit(org, project, n)),
        labels: f["System.Tags"]
            .as_str()
            .unwrap_or("")
            .split(';')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect(),
        assignee: who["displayName"].as_str().map(String::from),
        mine: me.is_some_and(|me| {
            who["uniqueName"]
                .as_str()
                .is_some_and(|u| u.eq_ignore_ascii_case(me))
        }),
        parent: None,
        depth: 0,
    }
}

fn ado_epic(org: &str, project: &str, parent: &Value, items: Vec<Item>) -> Epic {
    let id = parent["id"].as_u64().unwrap_or(0);
    let mut needles = vec![];
    for (n, item) in std::iter::once((id, None)).chain(
        items
            .iter()
            .filter_map(|i| Some((i.key.parse().ok()?, Some(i.key.clone())))),
    ) {
        needles.push(Needle {
            text: format!("_workitems/edit/{n}"),
            item: item.clone(),
        });
        needles.push(Needle {
            text: format!("AB#{n}"),
            item,
        });
    }
    Epic {
        title: parent["fields"]["System.Title"]
            .as_str()
            .unwrap_or("Untitled work item")
            .to_string(),
        url: ado_edit(org, project, id),
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
    fn explains_failures() {
        assert_eq!(
            failure("gh", "gh: Not Found (HTTP 404)\n"),
            "Not found: it may have been deleted or moved, or your sign-in can't see it"
        );
        assert_eq!(
            failure(
                "gh",
                "To get started with GitHub CLI, please run:  gh auth login\n"
            ),
            "Not signed in: run `gh auth login`"
        );
        assert_eq!(
            failure("az", "ERROR: Please run 'az login' to setup account.\n"),
            "Not signed in: run `az login`"
        );
        assert_eq!(
            failure("gh", "gh: rate limit exceeded"),
            "gh: rate limit exceeded"
        );
    }

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
        let sub = |n: u64, state: &str, reason: &str, label: &str, who: &[&str]| {
            json!({"number": n, "title": format!("Item {n}"), "state": state, "state_reason": reason,
                "html_url": format!("https://github.com/o/r/issues/{n}"),
                "labels": [{"name": label}],
                "assignees": who.iter().map(|w| json!({"login": w})).collect::<Vec<_>>()})
        };
        let items = vec![
            github_item(&sub(28, "closed", "completed", "agent", &[]), Some("me")),
            github_item(&sub(34, "open", "", "Human", &["Me", "sam"]), Some("me")),
            github_item(&sub(35, "closed", "not_planned", "", &[]), Some("me")),
            github_item(&sub(36, "open", "", "blocked", &["sam"]), Some("me")),
        ];
        assert_eq!(
            items
                .iter()
                .map(|i| (i.key.as_str(), i.status, i.mine))
                .collect::<Vec<_>>(),
            [
                ("28", TaskStatus::Done, false),
                ("34", TaskStatus::Pending, true),
                ("35", TaskStatus::Cancelled, false),
                ("36", TaskStatus::Blocked, false)
            ]
        );
        assert_eq!(items[1].assignee.as_deref(), Some("Me, sam"));
        // A moved issue is named by where it is now.
        let issue = json!({"number": 27, "title": "Explorer", "state": "closed",
            "html_url": "https://github.com/o/new/issues/27"});
        let e = github_epic("o", "r", &issue, items);
        assert!(e.closed());
        assert_eq!(e.repo.as_deref(), Some("o/new"));
        assert_eq!(e.numbers, [(27, None)]);
        assert!(
            e.needles
                .iter()
                .any(|n| n.text == "o/r#34" && n.item.as_deref() == Some("34"))
        );

        // Without sub-issues, the description's task list.
        let items = task_list(
            "Steps:\n- [x] Write  Spec\n- [ ] #5\n* [ ] Ship it\n- [?] no",
            "o",
            "r",
        );
        assert_eq!(
            items
                .iter()
                .map(|i| (i.key.as_str(), i.status, i.url.is_some()))
                .collect::<Vec<_>>(),
            [
                ("write spec", TaskStatus::Done, false),
                ("5", TaskStatus::Pending, true),
                ("ship it", TaskStatus::Pending, false)
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
        let org = "https://dev.azure.com/c";
        let a = ado_item(
            org,
            "My App",
            &json!({"id": 11, "fields": {"System.Title": "Swap SDK",
            "System.State": "Closed", "System.Tags": "agent; backend"}}),
            Some("sam@x.com"),
        );
        let b = ado_item(
            org,
            "My App",
            &json!({"id": 12, "fields": {"System.Title": "Approve rollout",
            "System.State": "New", "System.AssignedTo": {"displayName": "Sam", "uniqueName": "Sam@x.com"}}}),
            Some("sam@x.com"),
        );
        assert_eq!(
            (a.status, a.labels.clone(), a.mine),
            (
                TaskStatus::Done,
                vec!["agent".to_string(), "backend".into()],
                false
            )
        );
        assert_eq!((b.assignee.as_deref(), b.mine), (Some("Sam"), true));
        let e = ado_epic(org, "My App", &parent, vec![a, b]);
        assert_eq!(e.title, "Checkout");
        assert_eq!(e.status, TaskStatus::InProgress);
        assert_eq!(e.url, "https://dev.azure.com/c/My%20App/_workitems/edit/10");
        assert!(
            e.needles
                .iter()
                .any(|n| n.text == "AB#12" && n.item.as_deref() == Some("12"))
        );
    }

    #[test]
    fn renders_comments_for_azure_devops() {
        assert_eq!(
            markdown_to_html("### Progress\n6 of 8 done <ok>\n- #28 done\n- #34 you\n\nThanks"),
            "<h3>Progress</h3><p>6 of 8 done &lt;ok&gt;</p><ul><li>#28 done</li><li>#34 you</li></ul><p>Thanks</p>"
        );
    }

    /// Needs `gh` signed in and the network: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_a_real_github_epic() {
        let e = fetch(&parse("xanderlynn/tilly#27").unwrap()).unwrap();
        println!("{e:#?}");
        assert!(e.items.len() >= 8);
        assert_eq!(e.repo.as_deref(), Some("xanderlynn/tilly"));
    }
}
