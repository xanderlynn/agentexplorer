import { memo } from "react";
import type { Lane, Mode } from "../api";
import { MODE_COLOR, MODE_LABEL, SOURCE_COLOR, SPAN_MODE, duration, projectName } from "../format";

/** Waiting on you is hatched as well as amber, so it reads without colour. */
export const WAIT_FILL = "repeating-linear-gradient(135deg, var(--color-amber-500) 0 3px, color-mix(in oklab, var(--color-amber-500) 35%, transparent) 3px 6px)";

/**
 * One lane per session: agent work coloured by mode, and waiting on you (amber)
 * across the day, with a "now" line. Shows at a glance when parallel
 * agents were idling on you.
 */
export const Swimlanes = memo(function Swimlanes({ lanes, since, onOpen }: { lanes: Lane[]; since: number; onOpen: (id: string) => void }) {
  const now = Date.now();
  const start = Math.min(since, ...lanes.flatMap((l) => l.spans.map((s) => s[0])));
  const span = Math.max(1, now - start);
  const x = (t: number) => `${((Math.max(t, start) - start) / span) * 100}%`;
  const w = (a: number, b: number) => `${Math.max(0.2, ((Math.min(b, now) - Math.max(a, start)) / span) * 100)}%`;
  const hours: number[] = [];
  const first = new Date(start);
  first.setMinutes(0, 0, 0);
  for (let t = first.getTime() + 3_600_000; t < now; t += 3_600_000) hours.push(t);

  return (
    <div>
      <div className="relative ml-[9.5rem] h-4 text-xs text-zinc-500">
        {hours.map((t) => (
          <span key={t} className="absolute -translate-x-1/2" style={{ left: x(t) }}>
            {new Date(t).getHours()}
          </span>
        ))}
      </div>
      <ul className="space-y-1.5">
        {lanes.map((l) => {
          const working = l.spans.filter((s) => s[2] !== 0).reduce((a, s) => a + s[1] - s[0], 0);
          const waiting = l.spans.filter((s) => s[2] === 0).reduce((a, s) => a + s[1] - s[0], 0);
          return (
            <li key={l.id}>
              <button className="flex w-full items-center gap-2 text-left" onClick={() => onOpen(l.id)} title={`${duration(working)} working, ${duration(waiting)} waiting on you`}>
                <span className="w-36 shrink-0 truncate text-xs text-zinc-300">
                  <span className="mr-1.5 inline-block h-2 w-2 rounded-full" style={{ background: SOURCE_COLOR[l.source] }} aria-hidden />
                  {l.title ?? projectName(l.projectPath)}
                </span>
                <span className="relative h-4 flex-1 rounded bg-zinc-800/60">
                  {l.spans.map(([a, b, k], i) => (
                    <span
                      key={i}
                      className="absolute top-0.5 h-3 rounded-sm"
                      style={{ left: x(a), width: w(a, b), background: k === 0 ? WAIT_FILL : MODE_COLOR[SPAN_MODE[k] as Mode], opacity: k === 0 ? 0.85 : 0.9 }}
                      title={SPAN_MODE[k] === "waiting" ? "waiting on you" : MODE_LABEL[SPAN_MODE[k] as Mode]}
                    />
                  ))}
                  <span className="absolute -top-0.5 h-5 w-px bg-zinc-300" style={{ left: "100%" }} aria-hidden />
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-zinc-400">
        {(Object.keys(MODE_LABEL) as Mode[]).map((m) => (
          <span key={m} className="inline-flex items-center gap-1.5">
            <span className="h-2.5 w-2.5 rounded-sm" style={{ background: MODE_COLOR[m] }} /> {MODE_LABEL[m]}
          </span>
        ))}
        <span className="inline-flex items-center gap-1.5">
          <span className="h-2.5 w-2.5 rounded-sm" style={{ background: WAIT_FILL }} /> waiting on you
        </span>
      </div>
    </div>
  );
});
