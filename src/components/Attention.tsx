import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api } from "../api";
import { duration } from "../format";
import { chart } from "../theme";
import { Card, ErrorNote } from "./ui";

/**
 * Where your attention goes: agent time spent waiting on you, split into
 * short waits (replying) and away spells (over 30 minutes: meetings, the
 * night), by hour of the day and by project.
 */
export function AttentionCard({ days }: { days: number }) {
  const q = useQuery({ queryKey: ["attention", days], queryFn: () => api.attention(days) });
  const a = q.data;
  if (q.error) return <ErrorNote error={q.error} />;
  if (!a || a.waits === 0) return null;
  const total = a.shortMs + a.awayMs;
  const awayShare = total ? Math.round((a.awayMs / total) * 100) : 0;
  const data = a.byHour.map((h) => ({ hour: `${h.hour}:00`, Replying: h.shortMs / 3_600_000, Away: h.awayMs / 3_600_000 }));
  const headline =
    awayShare >= 40
      ? `${awayShare}% of the time agents waited on you was while you were away (${a.awaySpells} spells over 30 minutes). Before a meeting, give running agents enough to go on, or batch your replies just before you step out.`
      : `Most waiting is short replies (typical ${a.medianReplyMs ? duration(a.medianReplyMs) : "—"}). To get more done, run more agents in parallel rather than replying faster.`;
  return (
    <Card title="Your attention" className="mb-4">
      <p className="text-sm text-zinc-300">{headline}</p>
      <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2 text-sm sm:grid-cols-4">
        <div>
          <dt className="text-xs text-zinc-500">Agents waited on you</dt>
          <dd className="text-zinc-100">{duration(total)}</dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500">Replying (waits up to 30 min)</dt>
          <dd className="text-zinc-100">{duration(a.shortMs)}</dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500">While you were away</dt>
          <dd className="text-zinc-100">
            {duration(a.awayMs)} <span className="text-xs text-zinc-500">({awayShare}%)</span>
          </dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500">Waits longest at</dt>
          <dd className="text-zinc-100">{a.worstHour != null ? `${a.worstHour}:00` : "—"}</dd>
        </div>
      </dl>
      <div className="mt-3 h-40" role="img" aria-label="Hours agents waited on you, by hour of the day, replying and away">
        <ResponsiveContainer>
          <BarChart data={data} margin={{ left: 0, right: 8, top: 4, bottom: 0 }}>
            <XAxis dataKey="hour" {...chart.axisProps} interval={2} />
            <YAxis {...chart.axisProps} width={32} tickFormatter={(v) => `${Math.round(v)}h`} />
            <Tooltip {...chart.tooltip} formatter={(v) => `${(v as number).toFixed(1)}h`} />
            <Bar dataKey="Replying" stackId="w" fill="var(--color-sky-500)" isAnimationActive={false} />
            <Bar dataKey="Away" stackId="w" fill="var(--color-amber-500)" isAnimationActive={false} />
          </BarChart>
        </ResponsiveContainer>
      </div>
      {a.projects.length > 0 && (
        <p className="mt-2 text-xs text-zinc-500">
          Waiting most: {a.projects.map((p) => `${p.project} ${duration(p.waitMs)}`).join(" · ")}
        </p>
      )}
    </Card>
  );
}
