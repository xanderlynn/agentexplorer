import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CartesianGrid, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Source, type ToolkitItem, type ToolkitOutcomes } from "../api";
import { Card, Empty, ErrorNote, Heuristic, Legend, Loading, PageHeader, RANGE_OPTIONS, Rate, Segmented, Sparkline, Stat, cx } from "../components/ui";
import { SOURCE_COLOR, SOURCE_LABEL, SOURCE_OPTIONS, compact, pct, roleOf } from "../format";
import { MIN_N } from "../stats";
import { chart } from "../theme";

type Kind = "agents" | "tools" | "mcp" | "skills";

const KINDS: { value: Kind; label: string; noun: string; uses: string }[] = [
  { value: "agents", label: "Agents", noun: "agent role", uses: "Agents" },
  { value: "tools", label: "Tools", noun: "tool", uses: "Calls" },
  { value: "mcp", label: "MCP servers", noun: "MCP server", uses: "Calls" },
  { value: "skills", label: "Skills", noun: "skill", uses: "Runs" },
];

const LINE_COLORS = ["var(--color-emerald-500)", "#38bdf8", "#a78bfa", "#fb923c", "#f472b6"];

const KIND_HELP: Record<Kind, string> = {
  agents: "Sub-agents the lead handed work to, grouped by role. Claude Code and Copilot names for the same role are merged.",
  tools: "Built-in tools the agents called. MCP tools are counted under MCP servers.",
  mcp: "Calls to tools from each MCP server, by the lead or its sub-agents.",
  skills: "Skills the agent loaded and slash-command skills you ran yourself.",
};

function label(kind: Kind, key: string): string {
  if (kind === "agents") return roleOf(key)[0];
  // Connector servers are named by an id; keep it short.
  if (kind === "mcp" && /^[0-9a-f]{8}-[0-9a-f-]{27}$/.test(key)) return `connector ${key.slice(0, 8)}`;
  return key;
}

/**
 * Which agent roles, tools, MCP servers and skills you use, how that has
 * changed week to week, and how sessions that used each went compared with
 * sessions that did not.
 */
export default function Toolkit({ days, setDays }: { days: number; setDays: (d: number) => void }) {
  const [source, setSource] = useState<Source | "all">("all");
  const [kind, setKind] = useState<Kind>("agents");
  const q = useQuery({
    queryKey: ["toolkit", days, source],
    queryFn: () => api.toolkit(days, source === "all" ? null : source),
  });
  const header = (
    <PageHeader
      title="Toolkit"
      subtitle="The agents, tools, MCP servers and skills you work with: how often, how that is changing, and how those sessions went."
      actions={
        <div className="flex flex-wrap gap-2">
          <Segmented options={SOURCE_OPTIONS} value={source} onChange={setSource} />
          <Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />
        </div>
      }
    />
  );
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  const t = q.data;
  if (!t || t.sessions === 0) return <>{header}<Empty title="No sessions in range">Pick a longer range or another tool.</Empty></>;

  const info = KINDS.find((k) => k.value === kind)!;
  const items = t[kind];

  return (
    <>
      {header}
      <div className="space-y-6">
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-5">
          <Stat label="Sessions" value={t.sessions} />
          {KINDS.map((k) => (
            <Stat
              key={k.value}
              label={k.label}
              value={t[k.value].length}
              hint={t[k.value][0] ? `top: ${label(k.value, t[k.value][0].key)}` : "none used"}
              onClick={() => setKind(k.value)}
            />
          ))}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2">
          <Segmented options={KINDS.map((k) => ({ value: k.value, label: `${k.label} · ${t[k.value].length}` }))} value={kind} onChange={setKind} />
          <span className="text-xs text-zinc-500">{KIND_HELP[kind]}</span>
        </div>

        {items.length === 0 ? (
          <Empty title={`No ${info.noun}s used in range`}>
            {kind === "agents" && "None of these sessions handed work to a sub-agent."}
            {kind === "mcp" && "None of these sessions called an MCP server's tools."}
            {kind === "skills" && "None of these sessions ran a skill."}
          </Empty>
        ) : (
          <>
            <OverTime kind={kind} items={items} weeks={t.weeks} />
            <Card
              title={
                <span>
                  Against outcomes
                  <Heuristic title="Sessions that used it compared with sessions in the same range that did not. Success is the mean outcome of rated sessions; rework is the share of sessions with commits that were reverted or fixed within 7 days. A difference is a correlation, not proof: the sessions may differ in other ways." />
                </span>
              }
            >
              <ItemTable kind={kind} items={items} uses={info.uses} />
            </Card>
          </>
        )}
      </div>
    </>
  );
}

function OverTime({ kind, items, weeks }: { kind: Kind; items: ToolkitItem[]; weeks: string[] }) {
  const top = items.slice(0, 5);
  const data = weeks.map((w, i) => {
    const row: Record<string, string | number> = { week: w.slice(5) };
    for (const it of top) row[it.key] = it.weekly[i];
    return row;
  });
  return (
    <Card title="Sessions using each, per week">
      <div className="h-56">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart data={data} margin={{ left: -18, right: 8, top: 8 }}>
            <CartesianGrid stroke={chart.grid} vertical={false} />
            <XAxis dataKey="week" {...chart.axisProps} />
            <YAxis allowDecimals={false} {...chart.axisProps} />
            <Tooltip {...chart.tooltip} formatter={(v, name) => [v, label(kind, String(name))]} labelFormatter={(w) => `Week of ${w}`} />
            {top.map((it, i) => (
              <Line key={it.key} type="linear" dataKey={it.key} stroke={LINE_COLORS[i]} strokeWidth={2} dot={false} isAnimationActive={false} />
            ))}
          </LineChart>
        </ResponsiveContainer>
      </div>
      <div className="mt-2">
        <Legend items={top.map((it, i) => [label(kind, it.key), LINE_COLORS[i]])} />
      </div>
    </Card>
  );
}

function rework(o: ToolkitOutcomes) {
  if (o.shipped === 0) return <span className="text-xs text-zinc-500">—</span>;
  if (o.shipped < MIN_N) return <span className="text-xs tabular-nums text-zinc-500" title="Too few shipped sessions to compare">{o.reworked}/{o.shipped}</span>;
  return (
    <span className="text-xs tabular-nums text-zinc-300">
      {pct(o.reworked / o.shipped)} <span className="text-zinc-500">({o.shipped})</span>
    </span>
  );
}

/** Success with minus without, shown only when both sides have enough rated sessions. */
function Diff({ item }: { item: ToolkitItem }) {
  const a = item.with;
  const b = item.without;
  if (a.success == null || b.success == null || a.rated < MIN_N || b.rated < MIN_N) return <span className="text-xs text-zinc-600">—</span>;
  const d = a.success - b.success;
  return (
    <span className={cx("text-xs tabular-nums", d > 0.05 ? "text-emerald-300" : d < -0.05 ? "text-rose-300" : "text-zinc-400")}>
      {d > 0 ? "+" : ""}
      {Math.round(d * 100)} pts
    </span>
  );
}

function ItemTable({ kind, items, uses }: { kind: Kind; items: ToolkitItem[]; uses: string }) {
  const [all, setAll] = useState(false);
  const shown = all ? items : items.slice(0, 20);
  return (
    <div className="overflow-x-auto">
      <table className="w-full min-w-[46rem] text-sm">
        <thead className="text-xs text-zinc-500">
          <tr className="[&>th]:pb-2 [&>th]:font-normal">
            <th className="text-left">Name</th>
            <th className="text-right">Sessions</th>
            <th className="text-right">{uses}</th>
            <th className="pl-4 text-left">12 weeks</th>
            <th className="w-32 text-left">Success with</th>
            <th className="w-32 text-left">without</th>
            <th className="text-right">Diff</th>
            <th className="text-right">Rework with / without</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/60">
          {shown.map((it) => (
            <tr key={it.key} className="[&>td]:py-2">
              <td className="max-w-56">
                <div className="flex items-center gap-1.5">
                  {it.sources.map((s) => (
                    <span key={s} className="h-2 w-2 shrink-0 rounded-full" style={{ background: SOURCE_COLOR[s] }} title={SOURCE_LABEL[s]} />
                  ))}
                  <span className="truncate text-zinc-100" title={it.key}>{label(kind, it.key)}</span>
                </div>
                {kind === "agents" && label(kind, it.key) !== it.key && <div className="text-xs text-zinc-500">{it.key}</div>}
              </td>
              <td className="text-right tabular-nums text-zinc-300">{it.sessions}</td>
              <td className="text-right tabular-nums text-zinc-400">{compact(it.uses)}</td>
              <td className="pl-4"><Sparkline values={it.weekly} fromZero /></td>
              <td className="pr-3"><Rate rate={it.with.success} n={it.with.rated} /></td>
              <td className="pr-3"><Rate rate={it.without.success} n={it.without.rated} /></td>
              <td className="text-right"><Diff item={it} /></td>
              <td className="whitespace-nowrap text-right">
                {rework(it.with)} <span className="text-zinc-600">/</span> {rework(it.without)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {items.length > shown.length && (
        <button type="button" className="mt-2 text-xs text-zinc-400 hover:text-zinc-200" onClick={() => setAll(true)}>
          Show all {items.length}
        </button>
      )}
    </div>
  );
}
