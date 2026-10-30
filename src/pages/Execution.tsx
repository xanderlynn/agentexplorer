import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, CartesianGrid, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Mode, type ModeSplit } from "../api";
import type { Drill } from "../App";
import { Card, Empty, ErrorNote, Heuristic, Legend, Loading, PageHeader, RANGE_OPTIONS, Rate, Segmented, Stat } from "../components/ui";
import { MODE_COLOR, MODE_LABEL, STYLE_LABEL, compact, fixed, pct, sourceLabel } from "../format";
import { MIN_N } from "../stats";
import { chart } from "../theme";

const MODES: Mode[] = ["coding", "research", "running", "discussion"];

const MODE_HELP =
  "Each stretch of agent work is labelled by the tool that started it: edits are coding, commands are running, reads, searches and web lookups are research, and turns with no tools are discussion.";

const STYLE_HELP: Record<string, string> = {
  one_shot: "a single prompt",
  planned: "used plan mode, or came from a pre-flight plan",
  iterative: "several prompts without a plan",
};

export default function Execution({ days, setDays, drill }: { days: number; setDays: (d: number) => void; drill: (d: Drill) => void }) {
  const q = useQuery({ queryKey: ["execution", days], queryFn: () => api.execution(days) });
  const header = (
    <PageHeader
      title="Execution"
      subtitle="What the agents spend their time on, how you run sessions, and how often they outgrow the model's smart zone."
      actions={<Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />}
    />
  );
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  if (!q.data) return header;
  const e = q.data;
  const total = MODES.reduce((n, m) => n + e.modes[m], 0);
  const c = e.context;
  const zoneLabel = `${compact(c.smartZoneTokens)} tokens`;

  return (
    <>
      {header}

      <Card
        title={
          <span>
            Where agent time goes
            <Heuristic title={MODE_HELP} />
          </span>
        }
        action={<Legend items={MODES.map((m) => [MODE_LABEL[m], MODE_COLOR[m]])} />}
      >
        {total === 0 ? (
          <Empty title="No agent work in this range" />
        ) : (
          <>
            <ModeBar split={e.modes} />
            <div className="mt-3 grid grid-cols-2 gap-3 lg:grid-cols-4">
              {MODES.map((m) => (
                <div key={m} className="text-sm">
                  <span className="text-zinc-400">{MODE_LABEL[m]}</span>
                  <div className="text-lg font-semibold tabular-nums text-zinc-100">
                    {e.modes[m].toFixed(1)}h <span className="text-sm font-normal text-zinc-500">{pct(e.modes[m] / total)}</span>
                  </div>
                </div>
              ))}
            </div>
            <div className="mt-5 grid grid-cols-1 gap-4 xl:grid-cols-2">
              <div>
                <div className="mb-2 text-xs font-medium text-zinc-400">By tool</div>
                <ul className="space-y-2">
                  {e.bySource.map((s) => (
                    <li key={s.label}>
                      <div className="mb-1 text-xs text-zinc-300">{sourceLabel(s.label)}</div>
                      <ModeBar split={s.split} />
                    </li>
                  ))}
                </ul>
              </div>
              <div>
                <div className="mb-2 text-xs font-medium text-zinc-400">By week (hours)</div>
                <div className="h-44">
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={e.weekly.map((w) => ({ week: w.label, ...w.split }))} margin={{ left: -20, right: 8, top: 4 }}>
                      <CartesianGrid stroke={chart.grid} vertical={false} />
                      <XAxis dataKey="week" tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(d: string) => d.slice(5)} />
                      <YAxis tick={{ fill: chart.axis, fontSize: 12 }} />
                      <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} labelFormatter={(l) => `Week of ${l}`} formatter={(v, name) => [`${Number(v).toFixed(1)}h`, MODE_LABEL[name as Mode] ?? name]} />
                      {MODES.map((m) => (
                        <Bar isAnimationActive={false} key={m} dataKey={m} stackId="m" fill={MODE_COLOR[m]} />
                      ))}
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </div>
            </div>
          </>
        )}
      </Card>

      <Card title="How you run sessions" className="mt-4">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-zinc-500">
                <th className="pb-2 font-medium">Style</th>
                <th className="pb-2 text-right font-medium">Sessions</th>
                <th className="pb-2 text-right font-medium">Prompts</th>
                <th className="pb-2 text-right font-medium">Active min</th>
                <th className="pb-2 text-right font-medium">Tokens</th>
                <th className="pb-2 text-right font-medium">Corrections</th>
                <th className="pb-2 text-right font-medium">Reworked</th>
                <th className="w-40 pb-2 pl-4 font-medium">Success (rated)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {e.styles.map((s) => (
                <tr key={s.style} className="cursor-pointer hover:bg-zinc-800/30" onClick={() => s.ids.length && drill({ label: `${STYLE_LABEL[s.style]} sessions`, ids: s.ids })}>
                  <td className="py-1.5">
                    <span className="text-zinc-200">{STYLE_LABEL[s.style]}</span>
                    <span className="block text-xs text-zinc-500">{STYLE_HELP[s.style]}</span>
                  </td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{s.sessions}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{fixed(s.avgPrompts)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{s.avgActiveMinutes != null ? Math.round(s.avgActiveMinutes) : "—"}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{s.avgTokens != null ? compact(s.avgTokens) : "—"}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{fixed(s.avgCorrections)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{pct(s.reworkRate)}</td>
                  <td className="py-1.5 pl-4">
                    <Rate rate={s.successRate} n={s.rated} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="mt-2 text-xs text-zinc-500">
          Planning pays off when planned sessions need fewer corrections and get reworked less. Plan from Today or Journal, or use your tool's plan mode.
        </p>
      </Card>

      <Card title="Context and the smart zone" className="mt-4">
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Stat
            label="Past the smart zone"
            value={c.measured ? pct(c.overZone.length / c.measured) : "—"}
            n={c.measured}
            goodWhenUp={false}
            hint={`peak context above ${zoneLabel}`}
            onClick={c.overZone.length ? () => drill({ label: `sessions past ${zoneLabel}`, ids: c.overZone }) : undefined}
          />
          <Stat
            label="Compacted"
            value={c.measured ? pct(c.compacted.length / c.measured) : "—"}
            goodWhenUp={false}
            hint="context was summarised to keep going"
            onClick={c.compacted.length ? () => drill({ label: "compacted sessions", ids: c.compacted }) : undefined}
          />
          <Stat label="Success within" value={c.ratedWithin >= MIN_N ? pct(c.successWithin) : "—"} n={`${c.ratedWithin} rated`} />
          <Stat label="Success past it" value={c.ratedOver >= MIN_N ? pct(c.successOver) : "—"} n={`${c.ratedOver} rated`} />
        </div>
        <div className="mt-4 h-44">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={c.buckets.map(([label, sessions]) => ({ label, sessions }))} margin={{ left: -20, right: 8, top: 8 }}>
              <CartesianGrid stroke={chart.grid} vertical={false} />
              <XAxis dataKey="label" tick={{ fill: chart.axis, fontSize: 12 }} />
              <YAxis allowDecimals={false} tick={{ fill: chart.axis, fontSize: 12 }} />
              <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} formatter={(v) => [v, "Sessions"]} />
              <ReferenceLine x={zoneBucket(c.smartZoneTokens)} stroke="var(--color-amber-500)" strokeDasharray="4 3" />
              <Bar isAnimationActive={false} dataKey="sessions" fill={chart.accent} radius={[3, 3, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        </div>
        <p className="mt-2 text-xs text-zinc-500">
          Peak context per session. Past roughly {zoneLabel}, models follow instructions less reliably and lose earlier detail, well before the window is full. Change the limit in Settings. Keep sessions small, or start fresh with a summary.
        </p>
      </Card>
    </>
  );
}

/** A 100%-wide bar split by mode. */
function ModeBar({ split }: { split: ModeSplit }) {
  const total = MODES.reduce((n, m) => n + split[m], 0);
  if (!total) return <div className="h-3 rounded-full bg-zinc-800" />;
  return (
    <div className="flex h-3 overflow-hidden rounded-full" role="img" aria-label={MODES.map((m) => `${MODE_LABEL[m]} ${pct(split[m] / total)}`).join(", ")}>
      {MODES.map((m) => (
        <div key={m} style={{ width: `${(split[m] / total) * 100}%`, background: MODE_COLOR[m] }} title={`${MODE_LABEL[m]}: ${split[m].toFixed(1)}h (${pct(split[m] / total)})`} />
      ))}
    </div>
  );
}

/** The histogram bucket the smart-zone limit falls in, for the marker. */
function zoneBucket(tokens: number): string {
  if (tokens < 50_000) return "under 50k";
  if (tokens < 100_000) return "50–100k";
  if (tokens < 200_000) return "100–200k";
  if (tokens < 500_000) return "200–500k";
  return "500k+";
}
