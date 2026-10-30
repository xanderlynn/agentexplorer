//! A session's task list, as its agents keep it: Claude Code's `TodoWrite`
//! and `TaskCreate` / `TaskUpdate`, the Claude app's `update_plan`, Copilot
//! CLI's `todos` table (written through its `sql` tool) and VS Code's
//! `manage_todo_list`. Status changes and the agents' own progress notes
//! (Copilot's `report_intent`) become the task's update feed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Updates kept per session; the feed shows the latest.
const MAX_UPDATES: usize = 60;
const MAX_TITLE: usize = 200;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Pending,
    InProgress,
    /// The agent says it can't go on.
    Blocked,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 5] = [
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Blocked,
        TaskStatus::Done,
        TaskStatus::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<TaskStatus> {
        Some(match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "pending" | "not_started" | "todo" | "open" => TaskStatus::Pending,
            "blocked" | "waiting" | "on_hold" => TaskStatus::Blocked,
            "in_progress" | "active" | "doing" | "started" => TaskStatus::InProgress,
            "done" | "completed" | "complete" | "finished" => TaskStatus::Done,
            "cancelled" | "canceled" | "deleted" | "skipped" => TaskStatus::Cancelled,
            _ => return None,
        })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    /// The agent that last worked on it ("lead" or a sub-agent's id).
    pub agent: Option<String>,
    /// Tasks that must finish first.
    pub blocked_by: Vec<String>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskUpdate {
    pub at: Option<i64>,
    pub agent: String,
    /// The task it is about, when there is one.
    pub task: Option<String>,
    pub text: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskList {
    pub tasks: Vec<Task>,
    pub updates: Vec<TaskUpdate>,
    /// Claude Code numbers `TaskCreate` tasks 1, 2, 3… in creation order.
    #[serde(skip_serializing_if = "is_zero")]
    created: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn clip(s: &str) -> String {
    crate::util::truncate(s.trim(), MAX_TITLE)
}

impl TaskList {
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty() && self.updates.is_empty()
    }

    /// Applies a tool call by `agent`, if it is one that keeps a task list.
    pub fn apply_tool(&mut self, name: &str, input: &Value, at: Option<i64>, agent: &str) {
        let s = |k: &str| input.get(k).and_then(Value::as_str);
        match name.rsplit("__").next().unwrap_or(name) {
            "TodoWrite" => self.replace(
                input["todos"].as_array().into_iter().flatten().map(|t| {
                    let title = t["content"].as_str().unwrap_or_default();
                    (
                        t["id"].as_str().unwrap_or(title).to_string(),
                        title,
                        t["status"].as_str(),
                    )
                }),
                at,
                agent,
            ),
            "update_plan" => self.update_plan(input, at, agent),
            "manage_todo_list" => {
                if let Some(list) = input["todoList"].as_array() {
                    self.replace(
                        list.iter().map(|t| {
                            let id = match &t["id"] {
                                Value::Number(n) => n.to_string(),
                                v => v.as_str().unwrap_or_default().to_string(),
                            };
                            (
                                id,
                                t["title"].as_str().unwrap_or_default(),
                                t["status"].as_str(),
                            )
                        }),
                        at,
                        agent,
                    );
                }
            }
            "TaskCreate" => {
                self.created += 1;
                let id = self.created.to_string();
                let title = s("subject").or(s("description")).unwrap_or_default();
                self.upsert(&id, Some(title), Some(TaskStatus::Pending), at, agent);
            }
            "TaskUpdate" => {
                let Some(id) = s("taskId").map(str::to_string).or_else(|| {
                    input
                        .get("taskId")
                        .and_then(Value::as_u64)
                        .map(|n| n.to_string())
                }) else {
                    return;
                };
                let status = s("status").and_then(TaskStatus::parse);
                self.upsert(&id, s("subject"), status, at, s("owner").unwrap_or(agent));
                if let Some(deps) = input["addBlockedBy"].as_array()
                    && let Some(t) = self.tasks.iter_mut().find(|t| t.id == id)
                {
                    for d in deps {
                        let d = d
                            .as_str()
                            .map(str::to_string)
                            .or(d.as_u64().map(|n| n.to_string()));
                        if let Some(d) = d.filter(|d| !t.blocked_by.contains(d)) {
                            t.blocked_by.push(d);
                        }
                    }
                }
            }
            "sql" => {
                if let Some(q) = s("query") {
                    self.copilot_sql(q, at, agent);
                }
            }
            "report_intent" => {
                if let Some(text) = s("intent") {
                    let task = self.current().map(|t| t.id.clone());
                    self.note(at, agent, task, text);
                }
            }
            _ => {}
        }
    }

    /// The task being worked on now: the latest one in progress.
    fn current(&self) -> Option<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::InProgress)
            .max_by_key(|t| t.updated_at)
    }

    fn note(&mut self, at: Option<i64>, agent: &str, task: Option<String>, text: &str) {
        let text = clip(text);
        if text.is_empty()
            || self
                .updates
                .last()
                .is_some_and(|u| u.text == text && u.task == task)
        {
            return;
        }
        self.updates.push(TaskUpdate {
            at,
            agent: agent.to_string(),
            task,
            text,
        });
        if self.updates.len() > MAX_UPDATES {
            self.updates.drain(..self.updates.len() - MAX_UPDATES);
        }
    }

    /// Creates or changes a task, noting status changes in the feed.
    fn upsert(
        &mut self,
        id: &str,
        title: Option<&str>,
        status: Option<TaskStatus>,
        at: Option<i64>,
        agent: &str,
    ) {
        let i = match self.tasks.iter().position(|t| t.id == id) {
            Some(i) => i,
            None => {
                self.tasks.push(Task {
                    id: id.to_string(),
                    ..Default::default()
                });
                self.tasks.len() - 1
            }
        };
        let t = &mut self.tasks[i];
        if let Some(title) = title.map(clip).filter(|x| !x.is_empty()) {
            t.title = title;
        }
        let changed = status.is_some_and(|s| s != t.status);
        if let Some(s) = status {
            t.status = s;
        }
        if status.is_some() || title.is_some() {
            t.updated_at = at.or(t.updated_at);
        }
        if changed && status != Some(TaskStatus::Pending) {
            t.agent = Some(agent.to_string());
            let verb = match t.status {
                TaskStatus::InProgress => "Started",
                TaskStatus::Done => "Finished",
                TaskStatus::Cancelled => "Dropped",
                TaskStatus::Blocked => "Blocked",
                TaskStatus::Pending => unreachable!(),
            };
            let (id, text) = (t.id.clone(), format!("{verb}: {}", t.title));
            self.note(at, agent, Some(id), &text);
        }
    }

    /// A whole list written at once: tasks keep their identity (by id) so
    /// status changes are noticed; ones no longer listed are dropped.
    fn replace<'a>(
        &mut self,
        items: impl Iterator<Item = (String, &'a str, Option<&'a str>)>,
        at: Option<i64>,
        agent: &str,
    ) {
        let mut keep = Vec::new();
        for (id, title, status) in items {
            if id.is_empty() {
                continue;
            }
            self.upsert(
                &id,
                Some(title),
                status.and_then(TaskStatus::parse),
                at,
                agent,
            );
            keep.push(id);
        }
        self.tasks.retain(|t| keep.contains(&t.id));
        self.tasks
            .sort_by_key(|t| keep.iter().position(|k| *k == t.id).unwrap_or(usize::MAX));
    }

    /// The Claude app's plan tool: a whole list (`items`), or a step:
    /// `advance` (finish the current step, start the next), `done` and
    /// `start` (1-based positions).
    fn update_plan(&mut self, input: &Value, at: Option<i64>, agent: &str) {
        if let Some(items) = input["items"].as_array() {
            self.replace(
                items.iter().map(|t| {
                    let title = t["content"].as_str().unwrap_or_default();
                    (title.to_string(), title, t["status"].as_str())
                }),
                at,
                agent,
            );
            return;
        }
        let id_at = |l: &TaskList, n: u64| {
            l.tasks
                .get((n as usize).wrapping_sub(1))
                .map(|t| t.id.clone())
        };
        if input["advance"].as_bool() == Some(true) {
            let cur = self
                .tasks
                .iter()
                .position(|t| t.status == TaskStatus::InProgress);
            if let Some(i) = cur {
                let id = self.tasks[i].id.clone();
                self.upsert(&id, None, Some(TaskStatus::Done), at, agent);
            }
            let next = self
                .tasks
                .iter()
                .position(|t| t.status == TaskStatus::Pending);
            if let Some(i) = next {
                let id = self.tasks[i].id.clone();
                self.upsert(&id, None, Some(TaskStatus::InProgress), at, agent);
            }
        }
        for n in input["done"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
        {
            if let Some(id) = id_at(self, n) {
                self.upsert(&id, None, Some(TaskStatus::Done), at, agent);
            }
        }
        if let Some(id) = input["start"].as_u64().and_then(|n| id_at(self, n)) {
            self.upsert(&id, None, Some(TaskStatus::InProgress), at, agent);
        }
    }

    /// Copilot CLI keeps todos in SQL tables: `todos (id, title, …, status)`
    /// and `todo_deps (todo_id, depends_on)`.
    fn copilot_sql(&mut self, query: &str, at: Option<i64>, agent: &str) {
        for stmt in sql::statements(query) {
            match stmt {
                sql::Stmt::Insert {
                    table,
                    columns,
                    rows,
                } if table == "todos" => {
                    let col = |name: &str| columns.iter().position(|c| c == name);
                    let (Some(id), title, status) = (col("id"), col("title"), col("status")) else {
                        continue;
                    };
                    for row in rows {
                        let get = |i: Option<usize>| i.and_then(|i| row.get(i)).map(String::as_str);
                        if let Some(id) = get(Some(id)) {
                            let status = get(status)
                                .and_then(TaskStatus::parse)
                                .or(Some(TaskStatus::Pending));
                            self.upsert(id, get(title), status, at, agent);
                        }
                    }
                }
                sql::Stmt::Insert {
                    table,
                    columns,
                    rows,
                } if table == "todo_deps" => {
                    let col = |name: &str| columns.iter().position(|c| c == name);
                    let (Some(t), Some(d)) = (col("todo_id"), col("depends_on")) else {
                        continue;
                    };
                    for row in rows {
                        if let (Some(t), Some(d)) = (row.get(t), row.get(d))
                            && let Some(task) = self.tasks.iter_mut().find(|x| x.id == *t)
                            && !task.blocked_by.contains(d)
                        {
                            task.blocked_by.push(d.clone());
                        }
                    }
                }
                sql::Stmt::Update {
                    table,
                    status,
                    title,
                    ids,
                } if table == "todos" => {
                    for id in ids {
                        self.upsert(
                            &id,
                            title.as_deref(),
                            status.as_deref().and_then(TaskStatus::parse),
                            at,
                            agent,
                        );
                    }
                }
                sql::Stmt::Delete { table, ids } if table == "todos" => {
                    self.tasks.retain(|t| !ids.contains(&t.id));
                }
                _ => {}
            }
        }
    }
}

/// Just enough SQL to follow Copilot CLI's todo bookkeeping: `INSERT … VALUES`,
/// `UPDATE … SET status/title … WHERE id = … | id IN (…)` and
/// `DELETE … WHERE id …`. Anything else is ignored.
mod sql {
    #[derive(Debug, PartialEq)]
    pub enum Stmt {
        Insert {
            table: String,
            columns: Vec<String>,
            rows: Vec<Vec<String>>,
        },
        Update {
            table: String,
            status: Option<String>,
            title: Option<String>,
            ids: Vec<String>,
        },
        Delete {
            table: String,
            ids: Vec<String>,
        },
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Tok {
        Word(String),
        Str(String),
        Sym(char),
    }

    fn tokens(q: &str) -> Vec<Tok> {
        let mut out = vec![];
        let mut c = q.chars().peekable();
        while let Some(ch) = c.next() {
            match ch {
                '-' if c.peek() == Some(&'-') => {
                    for n in c.by_ref() {
                        if n == '\n' {
                            break;
                        }
                    }
                }
                '\'' => {
                    let mut s = String::new();
                    while let Some(n) = c.next() {
                        if n == '\'' {
                            if c.peek() == Some(&'\'') {
                                c.next();
                                s.push('\'');
                            } else {
                                break;
                            }
                        } else {
                            s.push(n);
                        }
                    }
                    out.push(Tok::Str(s));
                }
                ch if ch.is_alphanumeric() || ch == '_' => {
                    let mut w = ch.to_string();
                    while let Some(&n) = c.peek() {
                        if n.is_alphanumeric() || n == '_' || n == '.' {
                            w.push(n);
                            c.next();
                        } else {
                            break;
                        }
                    }
                    out.push(Tok::Word(w));
                }
                ch if ch.is_whitespace() => {}
                ch => out.push(Tok::Sym(ch)),
            }
        }
        out
    }

    fn word(t: Option<&Tok>) -> Option<String> {
        match t {
            Some(Tok::Word(w)) => Some(w.to_ascii_lowercase()),
            _ => None,
        }
    }

    fn value(t: Option<&Tok>) -> Option<String> {
        match t {
            Some(Tok::Str(s)) => Some(s.clone()),
            Some(Tok::Word(w)) => Some(w.clone()),
            _ => None,
        }
    }

    /// `( a , b , … )` starting at `i`; returns the values and the index after `)`.
    fn list(t: &[Tok], mut i: usize) -> Option<(Vec<String>, usize)> {
        if t.get(i) != Some(&Tok::Sym('(')) {
            return None;
        }
        i += 1;
        let mut out = vec![];
        while i < t.len() {
            match &t[i] {
                Tok::Sym(')') => return Some((out, i + 1)),
                Tok::Sym(',') => {}
                Tok::Sym('(') => {
                    // A nested call such as datetime('now'): keep its first value.
                    let (inner, j) = list(t, i)?;
                    out.push(inner.into_iter().next().unwrap_or_default());
                    i = j;
                    continue;
                }
                other => out.push(value(Some(other)).unwrap_or_default()),
            }
            i += 1;
        }
        None
    }

    /// `WHERE id = 'x'` or `WHERE id IN ('x', 'y')`.
    fn where_ids(t: &[Tok], i: usize) -> Vec<String> {
        let Some(w) = t[i..]
            .iter()
            .position(|x| word(Some(x)).as_deref() == Some("where"))
        else {
            return vec![];
        };
        let i = i + w + 1;
        if word(t.get(i)).as_deref() != Some("id") {
            return vec![];
        }
        match (t.get(i + 1), word(t.get(i + 1)).as_deref()) {
            (Some(Tok::Sym('=')), _) => value(t.get(i + 2)).into_iter().collect(),
            (_, Some("in")) => list(t, i + 2).map(|(v, _)| v).unwrap_or_default(),
            _ => vec![],
        }
    }

    pub fn statements(q: &str) -> Vec<Stmt> {
        let toks = tokens(q);
        toks.split(|t| *t == Tok::Sym(';'))
            .filter_map(statement)
            .collect()
    }

    fn statement(t: &[Tok]) -> Option<Stmt> {
        match word(t.first())?.as_str() {
            "insert" => {
                let into = t
                    .iter()
                    .position(|x| word(Some(x)).as_deref() == Some("into"))?;
                let table = word(t.get(into + 1))?;
                let (columns, mut i) = list(t, into + 2)?;
                if word(t.get(i)).as_deref() != Some("values") {
                    return None;
                }
                i += 1;
                let mut rows = vec![];
                while let Some((row, j)) = list(t, i) {
                    rows.push(row);
                    i = j;
                    if t.get(i) == Some(&Tok::Sym(',')) {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let columns = columns
                    .into_iter()
                    .map(|c| c.to_ascii_lowercase())
                    .collect();
                Some(Stmt::Insert {
                    table,
                    columns,
                    rows,
                })
            }
            "update" => {
                let table = word(t.get(1))?;
                let set = |name: &str| {
                    t.windows(3)
                        .find(|w| {
                            word(Some(&w[0])).as_deref() == Some(name) && w[1] == Tok::Sym('=')
                        })
                        .and_then(|w| value(Some(&w[2])))
                };
                Some(Stmt::Update {
                    table,
                    status: set("status"),
                    title: set("title"),
                    ids: where_ids(t, 2),
                })
            }
            "delete" => {
                let from = t
                    .iter()
                    .position(|x| word(Some(x)).as_deref() == Some("from"))?;
                Some(Stmt::Delete {
                    table: word(t.get(from + 1))?,
                    ids: where_ids(t, from + 2),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn titles(l: &TaskList) -> Vec<(String, TaskStatus)> {
        l.tasks
            .iter()
            .map(|t| (t.title.clone(), t.status))
            .collect()
    }

    #[test]
    fn copilot_todos_in_sql() {
        let mut l = TaskList::default();
        l.apply_tool(
            "sql",
            &json!({"query": "INSERT INTO todos (id, title, description, status) VALUES\n ('prd', 'Write the PRD', 'it''s long', 'pending'),\n ('api', 'Build the API', 'x', 'pending');\nINSERT INTO todo_deps (todo_id, depends_on) VALUES ('api', 'prd'); -- api waits"}),
            Some(1),
            "lead",
        );
        l.apply_tool(
            "sql",
            &json!({"query": "UPDATE todos SET status = 'in_progress' WHERE id = 'prd'"}),
            Some(2),
            "lead",
        );
        l.apply_tool(
            "report_intent",
            &json!({"intent": "Drafting requirements"}),
            Some(3),
            "lead",
        );
        l.apply_tool("sql", &json!({"query": "UPDATE todos SET status = 'done' WHERE id IN ('prd');\nUPDATE todos SET status='in_progress' WHERE id='api';"}), Some(4), "lead");
        l.apply_tool(
            "sql",
            &json!({"query": "SELECT id, title FROM todos"}),
            Some(5),
            "lead",
        );
        l.apply_tool(
            "sql",
            &json!({"query": "UPDATE review SET status = 'addressed' WHERE id = 'prd'"}),
            Some(6),
            "lead",
        );
        assert_eq!(
            titles(&l),
            [
                ("Write the PRD".into(), TaskStatus::Done),
                ("Build the API".into(), TaskStatus::InProgress)
            ]
        );
        assert_eq!(l.tasks[1].blocked_by, ["prd"]);
        let feed: Vec<_> = l
            .updates
            .iter()
            .map(|u| (u.task.as_deref(), u.text.as_str()))
            .collect();
        assert_eq!(
            feed,
            [
                (Some("prd"), "Started: Write the PRD"),
                (Some("prd"), "Drafting requirements"),
                (Some("prd"), "Finished: Write the PRD"),
                (Some("api"), "Started: Build the API"),
            ]
        );
    }

    #[test]
    fn claude_task_tools() {
        let mut l = TaskList::default();
        l.apply_tool(
            "TaskCreate",
            &json!({"subject": "Add the parser"}),
            Some(1),
            "lead",
        );
        l.apply_tool(
            "TaskCreate",
            &json!({"subject": "Write tests"}),
            Some(1),
            "lead",
        );
        l.apply_tool(
            "TaskUpdate",
            &json!({"taskId": "2", "addBlockedBy": ["1"]}),
            Some(2),
            "lead",
        );
        l.apply_tool(
            "TaskUpdate",
            &json!({"taskId": "1", "status": "in_progress", "owner": "worker"}),
            Some(3),
            "lead",
        );
        assert_eq!(l.tasks[0].agent.as_deref(), Some("worker"));
        assert_eq!(l.tasks[1].blocked_by, ["1"]);
        assert_eq!(l.tasks[0].status, TaskStatus::InProgress);
    }

    #[test]
    fn todo_write_and_plan_lists() {
        let mut l = TaskList::default();
        l.apply_tool(
            "TodoWrite",
            &json!({"todos": [{"content": "A", "status": "completed"}, {"content": "B", "status": "in_progress"}]}),
            Some(1),
            "lead",
        );
        assert_eq!(
            titles(&l),
            [
                ("A".into(), TaskStatus::Done),
                ("B".into(), TaskStatus::InProgress)
            ]
        );

        let mut p = TaskList::default();
        let plan = "mcp__ccd_turn__update_plan";
        p.apply_tool(plan, &json!({"items": [{"content": "One", "status": "in_progress"}, {"content": "Two", "status": "pending"}, {"content": "Three", "status": "pending"}]}), Some(1), "lead");
        p.apply_tool(plan, &json!({"advance": true}), Some(2), "lead");
        assert_eq!(
            p.tasks.iter().map(|t| t.status).collect::<Vec<_>>(),
            [
                TaskStatus::Done,
                TaskStatus::InProgress,
                TaskStatus::Pending
            ]
        );
        p.apply_tool(plan, &json!({"done": [2], "start": 3}), Some(3), "lead");
        assert_eq!(p.tasks[2].status, TaskStatus::InProgress);
        // A new list keeps known tasks by title and drops the rest.
        p.apply_tool(plan, &json!({"items": [{"content": "Three", "status": "completed"}, {"content": "Four", "status": "pending"}]}), Some(4), "lead");
        assert_eq!(
            titles(&p),
            [
                ("Three".into(), TaskStatus::Done),
                ("Four".into(), TaskStatus::Pending)
            ]
        );
    }

    #[test]
    fn vscode_todo_list() {
        let mut l = TaskList::default();
        l.apply_tool(
            "manage_todo_list",
            &json!({"operation": "write", "todoList": [{"id": 1, "title": "Read code", "status": "completed"}, {"id": 2, "title": "Fix bug", "status": "in-progress"}]}),
            Some(1),
            "lead",
        );
        assert_eq!(
            titles(&l),
            [
                ("Read code".into(), TaskStatus::Done),
                ("Fix bug".into(), TaskStatus::InProgress)
            ]
        );
    }
}
