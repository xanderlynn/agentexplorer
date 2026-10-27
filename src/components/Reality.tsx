import { useQuery } from "@tanstack/react-query";
import { api, type FitCell, type Source } from "../api";
import type { Drill } from "../App";
import { SOURCE_LABEL, TASK_TYPES, pct } from "../format";
import { MIN_N } from "../stats";
import { Card, Loading, Rate, Stat, cx } from "./ui";

const ANSWER_LABEL: Record<string, string> = {
  faster: "You said faster than by hand",
  same: "About the same",
  slower: "You said slower",
  unsure: "Not sure",
  unanswered: "Not answered",
};

/**
 * Checks self-reported success against what happened next: was "shipped"
 * work reverted or fixed soon after, and do sessions that felt faster hold
 * up? Developers reliably over-estimate how much AI speeds them up.
 */
export function RealityCheck({ days, drill }: { days: number; drill: (d: Drill) => void }) {
  const q = useQuery({ queryKey: ["reality", days], queryFn: () => api.reality(days) });
  if (q.isLoading) return <Loading />;
  const r = q.data;
  if (!r) return null;
  return (
    <Card title="Reality check" className="mt-6">
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat
          label="Shipped, then reworked"
          value={r.shippedWithCommits ? pct(r.reworkRate) : "—"}
          n={r.shippedWithCommits}
          goodWhenUp={false}
          hint={`${r.shippedReworked} of ${r.shippedWithCommits} shipped sessions with commits were reverted or fixed within 7 days`}
          onClick={() => drill({ label: "shipped sessions", outcome: "shipped" })}
        />
        <Stat
          label="Your estimates"
          value={r.timeRatio != null ? `${r.timeRatio.toFixed(2)}×` : "—"}
          n={r.timeRatioN}
          hint="actual ÷ expected time in your journal; above 1 means tasks take longer than you think"
        />
        <Stat
          label="Agent : you"
          value={r.leverage != null ? `${r.leverage.toFixed(1)}×` : "—"}
          hint={`${r.agentHours.toFixed(1)}h of agent work vs ${r.waitHours.toFixed(1)}h it waited on your replies`}
        />
        <Stat label="Felt faster" value={pct(feltFaster(r.byAnswer))} hint="of sessions where you answered" />
      </div>
      {r.byAnswer.length > 0 && (
        <div className="mt-4 overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-zinc-500">
                <th className="pb-2 font-medium">"Faster than by hand?"</th>
                <th className="pb-2 text-right font-medium">Sessions</th>
                <th className="pb-2 text-right font-medium">Active min</th>
                <th className="pb-2 text-right font-medium">Reworked later</th>
                <th className="w-40 pb-2 pl-4 font-medium">Success (rated)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {r.byAnswer.map((a) => (
                <tr key={a.answer}>
                  <td className="py-1.5 text-zinc-200">{ANSWER_LABEL[a.answer] ?? a.answer}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{a.sessions}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{a.avgActiveMinutes != null ? Math.round(a.avgActiveMinutes) : "—"}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">
                    {a.withCommits >= MIN_N ? pct(a.reworkRate) : <span className="text-zinc-500">{a.withCommits ? `n=${a.withCommits}` : "—"}</span>}
                  </td>
                  <td className="py-1.5 pl-4">
                    <Rate rate={a.successRate} n={a.rated} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-2 text-xs text-zinc-500">
            If "faster" sessions are reworked more often, the speed may not be real. Answer the question in the one-tap review to build this up.
          </p>
        </div>
      )}
    </Card>
  );
}

function feltFaster(rows: { answer: string; sessions: number }[]): number | null {
  const answered = rows.filter((r) => r.answer !== "unanswered" && r.answer !== "unsure");
  const n = answered.reduce((a, r) => a + r.sessions, 0);
  const f = rows.find((r) => r.answer === "faster")?.sessions ?? 0;
  return n ? f / n : null;
}

const VERDICT: Record<FitCell["verdict"], { label: string; cls: string }> = {
  delegate: { label: "Delegate", cls: "bg-emerald-500/15 text-emerald-300 ring-emerald-500/30" },
  structure: { label: "Delegate with structure", cls: "bg-amber-500/15 text-amber-300 ring-amber-500/30" },
  yourself: { label: "Consider doing it yourself", cls: "bg-rose-500/15 text-rose-300 ring-rose-500/30" },
  unknown: { label: "Not enough data", cls: "text-zinc-500 ring-zinc-700" },
};

/** Task type × tool: what to hand off, based on your own outcomes and rework. */
export function FitMatrix({ days, drill }: { days: number; drill: (d: Drill) => void }) {
  const q = useQuery({ queryKey: ["fit", days], queryFn: () => api.fit(days) });
  const cells = q.data ?? [];
  const sources = (Object.keys(SOURCE_LABEL) as Source[]).filter((s) => cells.some((c) => c.source === s));
  const cats = TASK_TYPES.filter((t) => cells.some((c) => c.category === t));
  return (
    <Card title="Delegation fit" className="mt-6">
      {q.isLoading && <Loading />}
      {cells.length === 0 && !q.isLoading && <p className="text-sm text-zinc-500">No sessions with signals in range.</p>}
      {cells.length > 0 && (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-zinc-500">
                <th className="pb-2 font-medium">Task type</th>
                {sources.map((s) => (
                  <th key={s} className="pb-2 pl-3 font-medium">{SOURCE_LABEL[s]}</th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {cats.map((cat) => (
                <tr key={cat}>
                  <td className="py-2 capitalize text-zinc-200">{cat}</td>
                  {sources.map((s) => {
                    const c = cells.find((x) => x.category === cat && x.source === s);
                    if (!c) return <td key={s} className="py-2 pl-3 text-xs text-zinc-600">—</td>;
                    const v = VERDICT[c.verdict];
                    return (
                      <td key={s} className="py-2 pl-3">
                        <button
                          className="text-left"
                          onClick={() => drill({ label: `${cat} with ${SOURCE_LABEL[s]}`, ids: c.ids })}
                          title={`${c.rated} rated of ${c.sessions}; success ${pct(c.successRate)}; rework ${pct(c.reworkRate)}`}
                        >
                          <span className={cx("inline-block rounded-md px-2 py-0.5 text-xs font-medium ring-1 ring-inset", v.cls)}>{v.label}</span>
                          <span className="mt-0.5 block text-xs tabular-nums text-zinc-500">
                            {c.rated >= MIN_N ? `${pct(c.successRate)} success` : `${c.rated}/${c.sessions} rated`}
                            {c.reworkRate != null && c.rated >= MIN_N && ` · ${pct(c.reworkRate)} rework`}
                          </span>
                        </button>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="mt-2 text-xs text-zinc-500">
        Delegate: at least 70% success and little rework. With structure: 45–70%, so give it a spec, a failing test or smaller steps. Needs 5 rated sessions per cell.
      </p>
    </Card>
  );
}

export function ToolHealthCard({ days }: { days: number }) {
  const q = useQuery({ queryKey: ["toolHealth", days], queryFn: () => api.toolHealth(days) });
  const rows = q.data ?? [];
  return (
    <Card title="Tool reliability and your vetoes">
      {q.isLoading && <Loading />}
      {rows.length === 0 && !q.isLoading && <p className="text-sm text-zinc-500">No tool failures or rejections in range.</p>}
      {rows.length > 0 && (
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs text-zinc-500">
              <th className="pb-2 font-medium">Tool</th>
              <th className="pb-2 text-right font-medium">Calls</th>
              <th className="pb-2 text-right font-medium">Failed</th>
              <th className="pb-2 text-right font-medium">You rejected</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-zinc-800/60">
            {rows.map((t) => (
              <tr key={t.tool}>
                <td className="py-1.5 font-mono text-xs text-zinc-200">{t.tool}</td>
                <td className="py-1.5 text-right tabular-nums text-zinc-300">{t.calls}</td>
                <td className={cx("py-1.5 text-right tabular-nums", (t.errorRate ?? 0) > 0.15 ? "text-amber-300" : "text-zinc-300")}>
                  {t.errors} {t.calls ? <span className="text-zinc-500">({pct(t.errorRate)})</span> : null}
                </td>
                <td className="py-1.5 text-right tabular-nums text-zinc-300">{t.vetoes || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className="mt-2 text-xs text-zinc-500">
        Failures usually mean setup the agent can't see (missing commands, flaky tests). Rejections are rules worth writing into CLAUDE.md or AGENTS.md.
      </p>
    </Card>
  );
}
