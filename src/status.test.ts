import { describe, expect, it } from "vitest";
import { STATUS_GROUP, type LiveStatus } from "./generated";
import { AGENT_STATUS, STATUS, isActive, isAwaiting, mostUrgent, urgency, type StatusKey } from "./status";

const LIVE = Object.keys(STATUS_GROUP) as LiveStatus[];

describe("status tokens", () => {
  it("give each session status the group live.rs gives it", () => {
    for (const s of LIVE) expect(STATUS[s].group, s).toBe(STATUS_GROUP[s]);
  });

  it("rank every status by urgency", () => {
    for (const k of Object.keys(STATUS) as StatusKey[]) expect(urgency(k), k).toBeGreaterThanOrEqual(0);
  });

  it("cover every sub-agent status", () => {
    for (const k of ["running", "quiet", "done", "failed", "stopped"] as const) expect(AGENT_STATUS[k].label).toBeTruthy();
  });
});

describe("urgency", () => {
  it("puts what needs you before what is running", () => {
    expect(mostUrgent(["working", "done", "asking"])).toBe("asking");
    expect(mostUrgent(["long_task", "needs_permission", "failed"])).toBe("needs_permission");
    expect(mostUrgent([])).toBeUndefined();
  });

  it("does not reorder its input", () => {
    const keys: StatusKey[] = ["done", "asking"];
    mostUrgent(keys);
    expect(keys).toEqual(["done", "asking"]);
  });
});

describe("groups", () => {
  it("match the control center's columns", () => {
    expect(LIVE.filter(isAwaiting).sort()).toEqual(["asking", "failed", "maybe_blocked", "needs_permission"]);
    expect(LIVE.filter(isActive).sort()).toEqual(["long_task", "working"]);
  });
});
