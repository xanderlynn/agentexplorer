import { useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FlaskConical, Target, Trash2 } from "lucide-react";
import { api, type Experiment, type GoalStatus, type MetricDef, type Source } from "../api";
import type { Drill } from "../App";
import { Button, Card, Empty, ErrorNote, Field, Loading, PageHeader, cx, inputClass } from "../components/ui";
import { SOURCE_LABEL, TASK_TYPES, date } from "../format";
import { fmtMetric } from "../metrics";

const READING: Record<Experiment["reading"], { label: string; cls: string }> = {
  too_early: { label: "Too early to tell", cls: "text-zinc-400" },
  better: { label: "▲ Better", cls: "text-emerald-300" },
  worse: { label: "▼ Worse", cls: "text-rose-300" },
  no_change: { label: "≈ No clear change", cls: "text-amber-300" },
};

export default function Experiments({ drill }: { drill: (d: Drill) => void }) {
  const exps = useQuery({ queryKey: ["experiments"], queryFn: api.experiments });
  const goals = useQuery({ queryKey: ["goals"], queryFn: api.goals });
  const metrics = useQuery({ queryKey: ["metrics"], queryFn: api.metrics });
  const [creating, setCreating] = useState(false);
  const active = exps.data?.filter((e) => e.status === "active") ?? [];
  const done = exps.data?.filter((e) => e.status !== "active") ?? [];

  return (
    <>
      <PageHeader
        title="Experiments & goals"
        subtitle="Change one habit at a time and measure it against a baseline locked when you start. Goals give direction; each has a counter-metric so it can't be met by making something else worse."
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            <FlaskConical size={15} /> New experiment
          </Button>
        }
      />
      {creating && metrics.data && <NewExperiment metrics={metrics.data} onDone={() => setCreating(false)} />}

      <h2 className="mb-3 text-sm font-medium text-zinc-300">Goals</h2>
      {goals.isLoading && <Loading />}
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-3">
        {goals.data?.map((g) => <GoalCard key={g.id} g={g} />)}
        {metrics.data && (goals.data?.length ?? 0) < 3 && <NewGoal metrics={metrics.data} />}
      </div>

      <h2 className="mb-3 mt-8 text-sm font-medium text-zinc-300">Running</h2>
      {exps.isLoading && <Loading />}
      <ErrorNote error={exps.error} />
      {exps.data && active.length === 0 && (
        <Empty title="No experiment running">Adopt one from your weekly retro, or start one here. One or two at a time is plenty.</Empty>
      )}
      <div className="space-y-3">
        {active.map((e) => <ExperimentCard key={e.id} e={e} drill={drill} />)}
      </div>

      {done.length > 0 && (
        <>
          <h2 className="mb-3 mt-8 text-sm font-medium text-zinc-300">Finished</h2>
          <div className="space-y-3">{done.map((e) => <ExperimentCard key={e.id} e={e} drill={drill} />)}</div>
        </>
      )}
    </>
  );
}

function ExperimentCard({ e, drill }: { e: Experiment; drill: (d: Drill) => void }) {
  const qc = useQueryClient();
  const [note, setNote] = useState("");
  const verdict = useMutation({
    mutationFn: (d: "keep" | "drop" | "extend") => api.experimentVerdict(e.id, d, note || null),
    onSuccess: () => qc.invalidateQueries(),
  });
  const del = useMutation({ mutationFn: () => api.deleteExperiment(e.id), onSuccess: () => qc.invalidateQueries() });
  const over = Date.now() > e.endAt;
  const r = READING[e.reading];
  const scope = [e.category && `${e.category} tasks`, e.source && SOURCE_LABEL[e.source]].filter(Boolean).join(" · ") || "all sessions";

  return (
    <Card>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-sm font-medium text-zinc-100">{e.title}</div>
          {e.hypothesis && <div className="mt-0.5 text-sm text-zinc-400">{e.hypothesis}</div>}
          <div className="mt-1 text-xs text-zinc-500">
            {e.metric.label} · {scope} · {date(e.startAt)} – {date(e.endAt)}
            {e.status !== "active" && ` · ${e.status}`}
          </div>
        </div>
        <Button size="sm" variant="ghost" onClick={() => del.mutate()} aria-label="Delete experiment">
          <Trash2 size={14} />
        </Button>
      </div>

      <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Metric label="Baseline" value={fmtMetric(e.metric, e.baselineValue)} hint={`n=${e.baselineN}, prior 4 weeks`} />
        <Metric label="Now" value={fmtMetric(e.metric, e.currentValue)} hint={`n=${e.currentN}`} />
        <Metric label="Reading" value={<span className={r.cls}>{r.label}</span>} hint={e.metric.higherIsBetter ? "higher is better" : "lower is better"} />
        <button className="text-left" onClick={() => drill({ label: `sessions in scope of "${e.title}"`, category: e.category ?? undefined, source: e.source ?? undefined })}>
          <Metric label="Applied" value={`${e.applied}/${e.eligible}`} hint="tag sessions in the drawer" />
        </button>
      </div>

      {e.status === "active" && (
        <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-zinc-800 pt-3">
          <input className={`${inputClass} min-w-[12rem] flex-1`} placeholder="What did you learn?" value={note} onChange={(ev) => setNote(ev.target.value)} />
          <Button size="sm" variant="primary" busy={verdict.isPending} onClick={() => verdict.mutate("keep")}>
            Keep the habit
          </Button>
          <Button size="sm" onClick={() => verdict.mutate("drop")}>Drop it</Button>
          <Button size="sm" variant="ghost" onClick={() => verdict.mutate("extend")}>
            {over ? "Extend a week" : "+1 week"}
          </Button>
        </div>
      )}
      {e.verdictNote && <p className="mt-2 text-sm italic text-zinc-400">“{e.verdictNote}”</p>}
      <ErrorNote error={verdict.error} />
    </Card>
  );
}

function Metric({ label, value, hint }: { label: string; value: ReactNode; hint: string }) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2">
      <div className="text-xs uppercase tracking-wide text-zinc-500">{label}</div>
      <div className="mt-0.5 text-sm font-semibold tabular-nums text-zinc-100">{value}</div>
      <div className="text-xs text-zinc-500">{hint}</div>
    </div>
  );
}

export function NewExperiment({
  metrics,
  onDone,
  initial,
}: {
  metrics: MetricDef[];
  onDone: () => void;
  initial?: { title: string; hypothesis: string | null; metric: string; category: string | null; retroId: number | null };
}) {
  const qc = useQueryClient();
  const [title, setTitle] = useState(initial?.title ?? "");
  const [hypothesis, setHypothesis] = useState(initial?.hypothesis ?? "");
  const [metric, setMetric] = useState(initial?.metric ?? "success_rate");
  const [category, setCategory] = useState(initial?.category ?? "");
  const [source, setSource] = useState<Source | "">("");
  const [days, setDays] = useState(14);
  const create = useMutation({
    mutationFn: () =>
      api.createExperiment({
        title,
        hypothesis: hypothesis || null,
        metric,
        category: category || null,
        source: source || null,
        days,
        retroId: initial?.retroId ?? null,
      }),
    onSuccess: () => {
      qc.invalidateQueries();
      onDone();
    },
  });
  return (
    <Card className="mb-6" title="New experiment">
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        <Field label="What will you do differently?">
          <input className={inputClass} value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Give the failing test before asking for a fix" />
        </Field>
        <Field label="Why you think it helps (optional)">
          <input className={inputClass} value={hypothesis} onChange={(e) => setHypothesis(e.target.value)} placeholder="Fewer corrections because the agent can check itself" />
        </Field>
        <Field label="Measure it by">
          <select className={inputClass} value={metric} onChange={(e) => setMetric(e.target.value)}>
            {metrics.map((m) => <option key={m.key} value={m.key}>{m.label}</option>)}
          </select>
        </Field>
        <div className="grid grid-cols-3 gap-3">
          <Field label="Tasks">
            <select className={inputClass} value={category} onChange={(e) => setCategory(e.target.value)}>
              <option value="">All</option>
              {TASK_TYPES.map((t) => <option key={t}>{t}</option>)}
            </select>
          </Field>
          <Field label="Tool">
            <select className={inputClass} value={source} onChange={(e) => setSource(e.target.value as Source | "")}>
              <option value="">Any</option>
              {(Object.keys(SOURCE_LABEL) as Source[]).map((s) => <option key={s} value={s}>{SOURCE_LABEL[s]}</option>)}
            </select>
          </Field>
          <Field label="Days">
            <input type="number" min={3} max={60} className={inputClass} value={days} onChange={(e) => setDays(Number(e.target.value))} />
          </Field>
        </div>
      </div>
      <p className="mt-2 text-xs text-zinc-500">The baseline is the same metric over your previous four weeks, locked now so it can't drift.</p>
      <div className="mt-3 flex gap-2">
        <Button variant="primary" busy={create.isPending} disabled={!title.trim()} onClick={() => create.mutate()}>
          Start experiment
        </Button>
        <Button variant="ghost" onClick={onDone}>Cancel</Button>
      </div>
      <ErrorNote error={create.error} />
    </Card>
  );
}

function GoalCard({ g }: { g: GoalStatus }) {
  const qc = useQueryClient();
  const retire = useMutation({ mutationFn: () => api.retireGoal(g.id), onSuccess: () => qc.invalidateQueries() });
  return (
    <Card>
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 text-sm font-medium text-zinc-100">
          <Target size={15} className="text-emerald-400" aria-hidden />
          {g.metric.label}
        </div>
        <button className="text-xs text-zinc-500 hover:text-zinc-300" onClick={() => retire.mutate()}>retire</button>
      </div>
      <div className="mt-2 flex items-baseline gap-2">
        <span className="text-2xl font-semibold tabular-nums text-zinc-50">{fmtMetric(g.metric, g.current)}</span>
        <span className="text-sm text-zinc-400">
          target {g.metric.higherIsBetter ? "≥" : "≤"} {fmtMetric(g.metric, g.target)}
        </span>
      </div>
      <div className={cx("text-xs", g.met == null ? "text-zinc-500" : g.met ? "text-emerald-300" : "text-amber-300")}>
        {g.met == null ? "No data this week" : g.met ? "✓ On target this week" : "! Not there yet this week"} · n={g.n}
      </div>
      <div className={cx("mt-2 border-t border-zinc-800 pt-2 text-xs", g.counterSlipping ? "text-rose-300" : "text-zinc-400")}>
        Guard: {g.counter.label} {fmtMetric(g.counter, g.counterCurrent)} (was {fmtMetric(g.counter, g.counterBaseline)})
        {g.counterSlipping && " ✕ slipping: the goal may be costing you here"}
      </div>
    </Card>
  );
}

function NewGoal({ metrics }: { metrics: MetricDef[] }) {
  const qc = useQueryClient();
  const [metric, setMetric] = useState("success_rate");
  const [target, setTarget] = useState("");
  const [counter, setCounter] = useState("rework_rate");
  const def = metrics.find((m) => m.key === metric);
  const create = useMutation({
    mutationFn: () => {
      const raw = Number(target);
      // Rates are entered as percentages.
      const value = def?.unit === "rate" ? raw / 100 : raw;
      return api.createGoal({ metric, target: value, counterMetric: counter });
    },
    onSuccess: () => {
      setTarget("");
      qc.invalidateQueries();
    },
  });
  return (
    <Card title="Add a goal">
      <div className="space-y-2">
        <select className={inputClass} value={metric} onChange={(e) => setMetric(e.target.value)} aria-label="Metric">
          {metrics.map((m) => <option key={m.key} value={m.key}>{m.label}</option>)}
        </select>
        <div className="flex items-center gap-2">
          <span className="text-sm text-zinc-400">{def?.higherIsBetter ? "at least" : "at most"}</span>
          <input type="number" className={inputClass} value={target} onChange={(e) => setTarget(e.target.value)} placeholder={def?.unit === "rate" ? "65 (%)" : "e.g. 1.5"} aria-label="Target" />
        </div>
        <select className={inputClass} value={counter} onChange={(e) => setCounter(e.target.value)} aria-label="Counter-metric">
          {metrics.filter((m) => m.key !== metric).map((m) => <option key={m.key} value={m.key}>Guard: {m.label}</option>)}
        </select>
        <Button size="sm" variant="primary" busy={create.isPending} disabled={!target} onClick={() => create.mutate()}>
          Add goal
        </Button>
        <ErrorNote error={create.error} />
      </div>
    </Card>
  );
}
