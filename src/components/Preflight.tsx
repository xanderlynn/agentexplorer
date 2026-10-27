import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, X } from "lucide-react";
import { api, type AskCheck, type Source } from "../api";
import { SOURCE_LABEL, TASK_TYPES, pct, projectName } from "../format";
import { MIN_N } from "../stats";
import { Button, ErrorNote, Field, cx, inputClass } from "./ui";

const CHECKS: { key: keyof Omit<AskCheck, "score">; label: string; hint: string }[] = [
  { key: "goal", label: "A goal", hint: "what you want, in a sentence or two" },
  { key: "doneCriteria", label: "Done when", hint: "what the result should do or look like" },
  { key: "anchors", label: "Anchors", hint: "files, functions, error messages" },
  { key: "constraints", label: "Constraints", hint: "what not to touch or change" },
  { key: "verification", label: "How to check", hint: "tests to run, a repro, a build" },
];

/**
 * Plan a task before handing it to an agent. Writing the plan is the same
 * act as writing a good opening prompt, so the journal entry costs nothing
 * extra and the prompt gets better.
 */
export function Preflight({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const [goal, setGoal] = useState("");
  const [doneWhen, setDoneWhen] = useState("");
  const [context, setContext] = useState("");
  const [tool, setTool] = useState<Source | "">("");
  const [projectPath, setProjectPath] = useState("");
  const [taskType, setTaskType] = useState<string | null>(null);
  const [expected, setExpected] = useState(30);
  const [confidence, setConfidence] = useState(3);
  const [copied, setCopied] = useState(false);

  const prompt = useMemo(() => {
    const parts = [goal.trim()];
    if (doneWhen.trim()) parts.push(`Done when: ${doneWhen.trim()}`);
    if (context.trim()) parts.push(`Context: ${context.trim()}`);
    return parts.filter(Boolean).join("\n\n");
  }, [goal, doneWhen, context]);

  // Debounce the advice lookup while typing.
  const [debounced, setDebounced] = useState(prompt);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(prompt), 350);
    return () => clearTimeout(t);
  }, [prompt]);
  const advice = useQuery({
    queryKey: ["preflight", debounced, tool],
    queryFn: () => api.preflight(debounced, tool || null),
    enabled: debounced.trim().length > 0,
  });
  const a = advice.data;
  const category = taskType ?? a?.category ?? "feature";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = useMutation({
    mutationFn: () =>
      api.createIntent({
        goal: goal.trim(),
        taskType: category,
        projectPath: projectPath || null,
        tool: tool || null,
        expectedMinutes: Math.max(1, Math.round(expected)),
        confidence,
        doneWhen: doneWhen || null,
        context: context || null,
      }),
    onSuccess: () => {
      qc.invalidateQueries();
      onClose();
    },
  });

  const copy = () =>
    navigator.clipboard.writeText(prompt).then(
      () => setCopied(true),
      () => setCopied(false),
    );

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-black/50 p-6" role="dialog" aria-modal="true" aria-label="Plan a task">
      <div className="w-full max-w-3xl rounded-xl border border-zinc-800 bg-zinc-950 shadow-2xl">
        <div className="flex items-center justify-between border-b border-zinc-800 px-5 py-3">
          <h2 className="text-base font-semibold text-zinc-50">Plan a task</h2>
          <Button variant="ghost" size="sm" onClick={onClose} aria-label="Close">
            <X size={16} />
          </Button>
        </div>
        <div className="grid grid-cols-1 gap-5 p-5 md:grid-cols-5">
          <div className="space-y-3 md:col-span-3">
            <Field label="Goal">
              <textarea className={inputClass} rows={3} value={goal} onChange={(e) => setGoal(e.target.value)} placeholder="Add CSV export to the reports page" autoFocus />
            </Field>
            <Field label="Done when">
              <textarea className={inputClass} rows={2} value={doneWhen} onChange={(e) => setDoneWhen(e.target.value)} placeholder="Export includes visible columns; existing tests pass; a new test covers empty reports" />
            </Field>
            <Field label="Context" hint="Files, tests, errors, and anything the agent should leave alone.">
              <textarea className={inputClass} rows={2} value={context} onChange={(e) => setContext(e.target.value)} placeholder="src/reports/page.tsx, src/reports/export.test.ts. Don't change the API." />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label="Type">
                <select className={inputClass} value={category} onChange={(e) => setTaskType(e.target.value)}>
                  {TASK_TYPES.map((t) => <option key={t}>{t}</option>)}
                </select>
              </Field>
              <Field label="Tool">
                <select className={inputClass} value={tool} onChange={(e) => setTool(e.target.value as Source | "")}>
                  <option value="">Any</option>
                  {(Object.keys(SOURCE_LABEL) as Source[]).map((s) => <option key={s} value={s}>{SOURCE_LABEL[s]}</option>)}
                </select>
              </Field>
            </div>
            <Field label="Project" hint="The next session started there links to this plan automatically.">
              <select className={inputClass} value={projectPath} onChange={(e) => setProjectPath(e.target.value)}>
                <option value="">None</option>
                {projects.data?.map((p) => <option key={p} value={p}>{projectName(p)} — {p}</option>)}
              </select>
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label="Expected minutes">
                <input type="number" min={1} className={inputClass} value={expected} onChange={(e) => setExpected(Math.max(1, Number(e.target.value)))} />
              </Field>
              <Field label={`Confidence: ${confidence}/5`}>
                <input type="range" min={1} max={5} value={confidence} onChange={(e) => setConfidence(Number(e.target.value))} className="mt-2 w-full accent-emerald-500" />
              </Field>
            </div>
          </div>

          <div className="space-y-4 md:col-span-2">
            <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-3">
              <div className="mb-2 flex items-baseline justify-between">
                <span className="text-sm font-medium text-zinc-200">Clear-ask checklist</span>
                <span className="text-sm tabular-nums text-zinc-300">{a ? `${a.ask.score}/5` : "—"}</span>
              </div>
              <ul className="space-y-1.5">
                {CHECKS.map((c) => {
                  const ok = !!a?.ask[c.key];
                  return (
                    <li key={c.key} className="flex gap-2 text-sm">
                      <span className={cx("mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full", ok ? "bg-emerald-500 text-[var(--color-on-accent)]" : "border border-zinc-600")}>
                        {ok && <Check size={11} strokeWidth={3} aria-label="present" />}
                      </span>
                      <span>
                        <span className={ok ? "text-zinc-200" : "text-zinc-400"}>{c.label}</span>
                        <span className="block text-xs text-zinc-500">{c.hint}</span>
                      </span>
                    </li>
                  );
                })}
              </ul>
            </div>

            <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-3 text-sm">
              <div className="mb-1 font-medium text-zinc-200">From your history</div>
              {!a ? (
                <p className="text-zinc-500">Start typing for advice based on your past sessions.</p>
              ) : (
                <ul className="space-y-1.5 text-zinc-400">
                  <li>
                    <span className="capitalize">{a.category}</span> tasks:{" "}
                    {a.categoryN >= MIN_N ? <span className="text-zinc-200">{pct(a.categorySuccess)} succeed</span> : <span>too few rated yet ({a.categoryN})</span>}
                  </li>
                  {a.clearN >= MIN_N && a.vagueN >= MIN_N && a.clearSuccess != null && a.vagueSuccess != null && (
                    <li>
                      With a clear ask: <span className="text-zinc-200">{pct(a.clearSuccess)}</span>; vague: <span className="text-zinc-200">{pct(a.vagueSuccess)}</span>
                    </li>
                  )}
                  {a.timeRatio != null && (
                    <li>
                      These usually take <span className="text-zinc-200">{a.timeRatio.toFixed(1)}×</span> your estimate.
                      {a.timeRatio > 1.2 && (
                        <button className="ml-1 text-emerald-400 hover:underline" onClick={() => setExpected(Math.round(expected * a.timeRatio!))}>
                          Use {Math.round(expected * a.timeRatio)} min
                        </button>
                      )}
                    </li>
                  )}
                </ul>
              )}
            </div>
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-zinc-800 px-5 py-3">
          <Button variant="ghost" onClick={copy} disabled={!prompt}>
            <Copy size={14} /> {copied ? "Copied" : "Copy as prompt"}
          </Button>
          <div className="flex items-center gap-2">
            <ErrorNote error={save.error} />
            <Button variant="primary" busy={save.isPending} disabled={!goal.trim()} onClick={() => save.mutate()}>
              Save plan
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
