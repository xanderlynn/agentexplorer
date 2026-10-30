import { describe, expect, it } from "vitest";
import { SOURCES } from "./generated";
import { SOURCE_COLOR, SOURCE_LABEL, SOURCE_OPTIONS, ago, appLabel, cronText, delta, duration, money, pct, projectName, roleOf, sourceLabel } from "./format";

describe("sources", () => {
  it("label, colour and offer every tool the engine supports", () => {
    for (const s of SOURCES) {
      expect(SOURCE_LABEL[s.id]).toBe(s.shortLabel);
      expect(SOURCE_COLOR[s.id], s.id).toMatch(/^#[0-9a-f]{6}$/);
      expect(SOURCE_OPTIONS.some((o) => o.value === s.id)).toBe(true);
    }
    expect(sourceLabel("something_new")).toBe("something_new");
    expect(appLabel("claude_code", "claude-desktop")).toBe("Claude app");
    expect(appLabel("copilot_cli", "copilot-app")).toBe("Copilot app");
    expect(appLabel("claude_code", "cli")).toBe(sourceLabel("claude_code"));
  });
});

describe("numbers", () => {
  it("formats durations", () => {
    expect(duration(null)).toBe("—");
    expect(duration(12_000)).toBe("12s");
    expect(duration(25 * 60_000)).toBe("25m");
    expect(duration(125 * 60_000)).toBe("2h 5m");
  });

  it("formats money and percentages", () => {
    expect(money(null)).toBe("—");
    expect(money(3.456)).toBe("$3.46");
    expect(money(3.456, true)).toBe("~$3.46");
    expect(pct(0.256)).toBe("26%");
    expect(pct(NaN)).toBe("—");
  });

  it("computes relative change", () => {
    expect(delta(150, 100)).toBe(0.5);
    expect(delta(50, -100)).toBe(1.5);
    expect(delta(1, 0)).toBeNull();
  });

  it("says how long ago", () => {
    expect(ago(30_000)).toBe("30s ago");
    expect(ago(5 * 60_000)).toBe("5m ago");
    expect(ago(3 * 3_600_000)).toBe("3h ago");
    expect(ago(72 * 3_600_000)).toBe("3d ago");
  });
});

describe("text", () => {
  it("names a project after its folder", () => {
    expect(projectName("/Users/demo/code/billing-api/")).toBe("billing-api");
    expect(projectName("C:\\work\\mobile-app")).toBe("mobile-app");
    expect(projectName(null)).toBe("—");
  });

  it("titles known and custom agent roles", () => {
    expect(roleOf("Explore")[0]).toBe("Scout");
    expect(roleOf("code-review agent")[0]).toBe("Reviewer");
    expect(roleOf("db_migrator")).toEqual(["Db migrator", "Custom agent"]);
  });

  it("reads common cron shapes", () => {
    expect(cronText("*/15 * * * *")).toBe("every 15 min");
    expect(cronText("0 9 * * *")).toBe("every day at 09:00");
    expect(cronText("30 7 * * 1-5")).toBe("weekdays at 07:30");
    expect(cronText("0 0 1 * *")).toBe("0 0 1 * *");
  });
});
