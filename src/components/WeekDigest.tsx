import { useQuery } from "@tanstack/react-query";
import { api, type Totals } from "../api";
import { duration, money, pct } from "../format";
import { Card } from "./ui";

/**
 * This week against last, in one card: no form, no AI run. What shipped, what
 * it cost, how long agents waited on you, and which workstreams finished.
 */
export function WeekDigest() {
  const o = useQuery({ queryKey: ["overview", 7], queryFn: () => api.overview(7) });
  const att = useQuery({ queryKey: ["attention", 7], queryFn: () => api.attention(7) });
  const ws = useQuery({ queryKey: ["workstreams", 7], queryFn: () => api.workstreams(7) });
  const t = o.data?.totals;
  if (!t || !t.sessions) return null;
  const p = o.data?.previous ?? null;
  const done = (ws.data ?? []).filter((w) => {
    const c = w.counts;
    const total = c.done + c.inProgress + c.pending;
    return total > 0 && c.done === total;
  });
  const vs = (now: number, before: number | null | undefined, f: (n: number) => string) =>
    before != null && before > 0 ? `${f(now)} (last week ${f(before)})` : f(now);
  const perShipped = (x: Totals) => (x.shipped ? x.costUsd / x.shipped : null);
  const a = att.data;
  return (
    <Card title="This week" className="mb-4">
      <ul className="space-y-1.5 text-sm text-zinc-300">
        <li>
          {vs(t.sessions, p?.sessions, String)} sessions, {t.shipped} shipped
          {t.rated ? `, success ${pct(t.successRate)}` : ""}.
        </li>
        <li>
          Cost {vs(t.costUsd, p?.costUsd, (n) => money(n))}
          {perShipped(t) != null ? `, ${money(perShipped(t))} per shipped session` : ""}.
        </li>
        {a && a.waits > 0 && (
          <li>
            Agents waited on you {duration(a.shortMs + a.awayMs)}: {duration(a.shortMs)} replying, {duration(a.awayMs)} while you were away
            {a.medianReplyMs ? `; typical reply ${duration(a.medianReplyMs)}` : ""}.
          </li>
        )}
        {done.length > 0 && (
          <li>
            Finished {done.length} workstream{done.length === 1 ? "" : "s"}: {done.slice(0, 5).map((w) => w.title).join(", ")}
            {done.length > 5 ? ` and ${done.length - 5} more` : ""}.
          </li>
        )}
      </ul>
    </Card>
  );
}
