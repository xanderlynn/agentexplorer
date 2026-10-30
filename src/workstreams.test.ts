import { describe, expect, it } from "vitest";
import type { Workstream, WsTask } from "./api";
import { initials, progress, sortForYou, taskState, trackerName, trackerTasks, tree } from "./workstreams";

const task = (over: Partial<WsTask>): WsTask => ({ key: "s:1", sessionId: "s", title: "T", status: "pending", kind: "agent", reason: null, agent: null, blockedBy: [], updatedAt: null, url: null, ...over });
const ws = (over: Partial<Workstream>): Workstream => ({
  key: "k",
  title: "W",
  starred: false,
  custom: false,
  sourceUrl: null,
  syncedAt: null,
  repo: null,
  branch: null,
  lastActivity: 0,
  sessions: [{ id: "s", source: "claude_code", title: null, status: "working", endedAt: 0 }],
  agents: [],
  tasks: [],
  updates: [],
  counts: { done: 0, inProgress: 0, needsYou: 0, pending: 0, agent: 0, human: 0 },
  ...over,
});

describe("workstreams", () => {
  it("splits progress into done, in progress and waiting on you", () => {
    const p = progress(ws({ tasks: [task({ status: "done" }), task({ status: "in_progress" }), task({ kind: "human" }), task({})] }));
    expect(p).toEqual({ done: 0.25, doing: 0.25, waiting: 0.25, total: 4 });
    expect(progress(ws({})).total).toBe(0);
  });

  it("gives each task one state, yours before blocked", () => {
    expect(taskState(task({ status: "done", kind: "human" }))).toBe("done");
    expect(taskState(task({ kind: "human", blockedBy: ["s:2"] }))).toBe("yours");
    expect(taskState(task({ blockedBy: ["s:2"], status: "in_progress" }))).toBe("blocked");
    expect(taskState(task({ status: "in_progress" }))).toBe("doing");
    expect(taskState(task({}))).toBe("todo");
  });

  it("builds the tree by session and drops empty sessions", () => {
    const w = ws({
      sessions: [
        { id: "a", source: "claude_code", title: "A", status: "done", endedAt: 1 },
        { id: "b", source: "copilot_cli", title: "B", status: "working", endedAt: 2 },
      ],
      tasks: [task({ sessionId: "b", key: "b:1" })],
    });
    expect(tree(w).map((n) => n.session.id)).toEqual(["b"]);
    // In one you made, every session you put in shows.
    expect(tree({ ...w, custom: true }).map((n) => n.session.id)).toEqual(["a", "b"]);
  });

  it("makes initials and puts starred, then what needs you, first", () => {
    expect(initials("Code reviewer")).toBe("CR");
    expect(initials("lead")).toBe("Le");
    const a = ws({ key: "a", lastActivity: 5 });
    const b = ws({ key: "b", lastActivity: 1, counts: { ...a.counts, needsYou: 2 } });
    expect(sortForYou([a, b]).map((w) => w.key)).toEqual(["b", "a"]);
    const c = ws({ key: "c", lastActivity: 0, starred: true });
    expect(sortForYou([a, b, c]).map((w) => w.key)).toEqual(["c", "b", "a"]);
  });

  it("keeps tracker items apart from session tasks", () => {
    const w = ws({ tasks: [task({}), task({ key: "item:k:12", sessionId: "" })] });
    expect(trackerTasks(w).map((t) => t.key)).toEqual(["item:k:12"]);
    expect(tree(w)[0].tasks.map((t) => t.key)).toEqual(["s:1"]);
    expect(trackerName("https://github.com/o/r/issues/1")).toBe("GitHub");
    expect(trackerName("https://dev.azure.com/o/p/_workitems/edit/1")).toBe("Azure DevOps");
  });
});
