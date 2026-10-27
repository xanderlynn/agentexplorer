import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link2, RotateCcw, Trash2 } from "lucide-react";
import { api, type CalibrationRow, type Intent, type Outcome, type Source } from "../api";
import { Preflight } from "../components/Preflight";
import { Button, Card, Empty, ErrorNote, Loading, OutcomeBadge, OutcomePicker, PageHeader, RANGE_OPTIONS, Segmented, Stat, inputBase, inputClass } from "../components/ui";
import { SOURCE_LABEL, dateTime, pct, projectName } from "../format";

export default function Journal({ days, setDays, onOpen }: { days: number; setDays: (d: number) => void; onOpen: (id: string) => void }) {
  const intents = useQuery({ queryKey: ["intents"], queryFn: api.intents });
  const cal = useQuery({ queryKey: ["calibration", days], queryFn: () => api.calibration(days) });
  const open = intents.data?.filter((i) => i.closedAt == null) ?? [];
  const closed = intents.data?.filter((i) => i.closedAt != null) ?? [];

  return (
    <>
      <PageHeader
        title="Journal"
        subtitle="Before you hand work to an agent, write down what you expect. Afterwards, record what happened. This is how you learn what to delegate."
        actions={<Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />}
      />
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
        <PlanCard />
        <Card title={`Open intents (${open.length})`} className="xl:col-span-2">
          {intents.isLoading && <Loading />}
          {intents.data && open.length === 0 && <p className="text-sm text-zinc-500">Nothing in flight. Log your next task on the left before you start it.</p>}
          <ul className="space-y-3">
            {open.map((i) => <OpenIntent key={i.id} intent={i} onOpen={onOpen} />)}
          </ul>
        </Card>
      </div>

      <h2 className="mb-3 mt-8 text-sm font-medium text-zinc-300">Calibration</h2>
      {cal.data && cal.data.closed === 0 ? (
        <Empty title="No closed intents yet">After a few tasks you'll see whether you underestimate, which task types you delegate well, and whether your confidence predicts success.</Empty>
      ) : (
        cal.data && (
          <>
            <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
              <Stat label="Closed intents" value={cal.data.closed} />
              <Stat label="Success rate" value={pct(cal.data.overall?.successRate)} />
              <Stat label="Actual ÷ expected time" value={cal.data.overall?.timeRatio != null ? `${cal.data.overall.timeRatio.toFixed(2)}×` : "—"} hint="above 1 means you underestimate" />
              <Stat label="Avg confidence" value={cal.data.overall ? `${cal.data.overall.avgConfidence.toFixed(1)}/5` : "—"} />
            </div>
            <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-3">
              <CalTable title="By task type" rows={cal.data.byTaskType} />
              <CalTable title="By confidence" rows={cal.data.byConfidence} keyLabel={(k) => `${k}/5`} />
              <CalTable title="By tool" rows={cal.data.byTool} keyLabel={(k) => SOURCE_LABEL[k as Source] ?? k} />
            </div>
          </>
        )
      )}

      <Card title={`History (${closed.length})`} className="mt-8">
        {closed.length === 0 ? (
          <p className="text-sm text-zinc-500">Closed intents appear here.</p>
        ) : (
          <ul className="divide-y divide-zinc-800/70">
            {closed.map((i) => <ClosedIntent key={i.id} intent={i} onOpen={onOpen} />)}
          </ul>
        )}
      </Card>
    </>
  );
}

function PlanCard() {
  const [open, setOpen] = useState(false);
  return (
    <Card title="Plan before you delegate">
      <p className="text-sm text-zinc-400">
        Write the goal, what done looks like, and the files involved. It becomes your opening prompt and a journal entry in one step, with advice from your own history.
      </p>
      <Button className="mt-3" variant="primary" onClick={() => setOpen(true)}>
        Plan a task
      </Button>
      {open && <Preflight onClose={() => setOpen(false)} />}
    </Card>
  );
}

function OpenIntent({ intent: i, onOpen }: { intent: Intent; onOpen: (id: string) => void }) {
  const qc = useQueryClient();
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [actual, setActual] = useState<string>("");
  const [reflection, setReflection] = useState("");
  const close = useMutation({
    mutationFn: () => api.closeIntent({ id: i.id, outcome: outcome!, actualMinutes: actual ? Math.max(1, Math.round(Number(actual))) : null, reflection: reflection || null }),
    onSuccess: () => qc.invalidateQueries(),
  });
  const del = useMutation({ mutationFn: () => api.deleteIntent(i.id), onSuccess: () => qc.invalidateQueries() });
  const link = useMutation({ mutationFn: (sessionId: string | null) => api.linkIntent(i.id, sessionId), onSuccess: () => qc.invalidateQueries() });
  const recent = useQuery({ queryKey: ["sessions", { days: 3 }], queryFn: () => api.sessions({ days: 3 }), enabled: !i.sessionId });
  const suggested = i.sessionActiveMinutes != null ? Math.round(i.sessionActiveMinutes) : null;

  return (
    <li className="rounded-lg border border-zinc-800 bg-zinc-950/40 p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-sm text-zinc-100">{i.goal}</div>
          <div className="mt-0.5 text-xs text-zinc-500">
            {i.taskType} · expected {i.expectedMinutes} min · confidence {i.confidence}/5 · {projectName(i.projectPath)} · logged {dateTime(i.createdAt)}
          </div>
          {i.sessionId ? (
            <div className="mt-1 flex items-center gap-2 text-xs">
              <button className="inline-flex items-center gap-1 text-emerald-400 hover:underline" onClick={() => onOpen(i.sessionId!)}>
                <Link2 size={12} /> {i.sessionTitle ?? "Linked session"} {suggested != null && `(${suggested} min active)`}
              </button>
              <button className="text-zinc-500 hover:text-zinc-300" onClick={() => link.mutate(null)}>unlink</button>
            </div>
          ) : (
            <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-zinc-500">
              <span>{i.projectPath ? "Waiting for a session in this project…" : "No project set."}</span>
              <select className={`${inputBase} py-0.5 text-xs`} value="" onChange={(e) => e.target.value && link.mutate(e.target.value)}>
                <option value="">Link a session…</option>
                {recent.data?.slice(0, 30).map((s) => (
                  <option key={s.id} value={s.id}>{dateTime(s.startedAt)} · {projectName(s.repoRoot ?? s.projectPath)} · {s.title ?? "Untitled"}</option>
                ))}
              </select>
            </div>
          )}
        </div>
        <Button size="sm" variant="ghost" onClick={() => del.mutate()} aria-label="Delete intent"><Trash2 size={14} /></Button>
      </div>
      <div className="mt-3 flex flex-wrap items-end gap-3 border-t border-zinc-800/80 pt-3">
        <OutcomePicker value={outcome} onChange={setOutcome} />
        <input type="number" min={1} className={`${inputClass} w-32`} placeholder={suggested != null ? `${suggested} (session)` : "Actual min"} value={actual} onChange={(e) => setActual(e.target.value)} />
        <input className={`${inputClass} min-w-[12rem] flex-1`} placeholder="What surprised you?" value={reflection} onChange={(e) => setReflection(e.target.value)} />
        <Button size="sm" variant="primary" disabled={!outcome} busy={close.isPending} onClick={() => close.mutate()}>Close</Button>
      </div>
      <ErrorNote error={close.error} />
    </li>
  );
}

function ClosedIntent({ intent: i, onOpen }: { intent: Intent; onOpen: (id: string) => void }) {
  const qc = useQueryClient();
  const reopen = useMutation({ mutationFn: () => api.reopenIntent(i.id), onSuccess: () => qc.invalidateQueries() });
  const actual = i.actualMinutes ?? (i.sessionActiveMinutes != null ? Math.round(i.sessionActiveMinutes) : null);
  const ratio = actual != null ? actual / i.expectedMinutes : null;
  return (
    <li className="flex items-start gap-3 py-2.5">
      <OutcomeBadge outcome={i.outcome} />
      <div className="min-w-0 flex-1">
        <div className="text-sm text-zinc-200">{i.goal}</div>
        <div className="mt-0.5 text-xs text-zinc-500">
          {i.taskType} · expected {i.expectedMinutes} min, took {actual != null ? `${actual} min` : "?"}
          {ratio != null && <span className={ratio > 1.3 ? " text-amber-400" : ""}> ({ratio.toFixed(1)}×)</span>} · confidence {i.confidence}/5
          {i.sessionId && (
            <button className="ml-2 text-emerald-400 hover:underline" onClick={() => onOpen(i.sessionId!)}>session</button>
          )}
        </div>
        {i.reflection && <div className="mt-1 text-sm italic text-zinc-400">“{i.reflection}”</div>}
      </div>
      <Button size="sm" variant="ghost" onClick={() => reopen.mutate()} aria-label="Reopen"><RotateCcw size={14} /></Button>
    </li>
  );
}

function CalTable({ title, rows, keyLabel = (k) => k }: { title: string; rows: CalibrationRow[]; keyLabel?: (k: string) => string }) {
  return (
    <Card title={title}>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-xs text-zinc-500">
            <th className="pb-2 font-medium" />
            <th className="pb-2 text-right font-medium">n</th>
            <th className="pb-2 text-right font-medium">Time ×</th>
            <th className="pb-2 text-right font-medium">Success</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/60">
          {rows.map((r) => (
            <tr key={r.key}>
              <td className="py-1.5 text-zinc-200">{keyLabel(r.key)}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-400">{r.n}</td>
              <td className={`py-1.5 text-right tabular-nums ${r.timeRatio != null && r.timeRatio > 1.3 ? "text-amber-300" : "text-zinc-300"}`}>{r.timeRatio != null ? `${r.timeRatio.toFixed(2)}×` : "—"}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-300">{pct(r.successRate)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Card>
  );
}
