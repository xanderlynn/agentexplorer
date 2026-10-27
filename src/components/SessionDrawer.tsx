import { useEffect, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Copy, FolderOpen, GitCommitHorizontal, X } from "lucide-react";
import { api, currentVersion, type Outcome, type VsManual } from "../api";
import { compact, dateTime, duration, money, projectName } from "../format";
import { Button, ErrorNote, Field, Heuristic, inputClass, Loading, OutcomePicker, Rating, SourceBadge } from "./ui";

export function SessionDrawer({ id, onClose }: { id: string; onClose: () => void }) {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["session", id], queryFn: () => api.session(id) });
  const playbooks = useQuery({ queryKey: ["playbooks"], queryFn: api.playbooks });

  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [rating, setRating] = useState<number | null>(null);
  const [notes, setNotes] = useState("");
  const [tags, setTags] = useState("");
  const [vsManual, setVsManual] = useState<VsManual | null>(null);
  const exps = useQuery({ queryKey: ["experiments"], queryFn: api.experiments });
  const applied = useMutation({
    mutationFn: (v: { experimentId: number; on: boolean }) => api.setExperimentApplied(id, v.experimentId, v.on),
    onSuccess: () => qc.invalidateQueries(),
  });
  const [loadedFor, setLoadedFor] = useState<string | null>(null);

  // Seed the form once per session, not on every refetch, so typing is not lost.
  useEffect(() => {
    if (q.data && loadedFor !== id) {
      setOutcome(q.data.session.outcome);
      setRating(q.data.session.rating);
      setNotes(q.data.notes ?? "");
      setTags(q.data.session.tags.filter((t) => !t.startsWith("exp-")).join(", "));
      setVsManual(q.data.session.vsManual);
      setLoadedFor(id);
    }
  }, [q.data, id, loadedFor]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = useMutation({
    mutationFn: () =>
      api.saveReview({
        id,
        outcome,
        rating,
        notes: notes.trim() || null,
        // Experiment tags are managed by the toggles below, not the text box.
        tags: [
          ...tags.split(",").map((t) => t.trim()).filter(Boolean),
          ...(q.data?.session.tags.filter((t) => t.startsWith("exp-")) ?? []),
        ],
        vsManual,
      }),
    onSuccess: () => qc.invalidateQueries(),
  });

  const togglePlaybook = useMutation({
    mutationFn: ({ versionId, linked }: { versionId: number; linked: boolean }) => api.setSessionPlaybook(id, versionId, linked),
    onSuccess: () => qc.invalidateQueries(),
  });

  const d = q.data;
  const s = d?.session;
  const templates = (playbooks.data ?? []).filter((p) => p.kind === "prompt" && p.versions.length > 0);
  const linkedVersions = new Set(d?.playbooks.map((p) => p.versionId));

  return (
    <div className="fixed inset-0 z-40 flex justify-end" role="dialog" aria-modal="true">
      <button aria-label="Close" className="absolute inset-0 bg-black/50" onClick={onClose} />
      <div className="relative flex h-full w-full max-w-2xl flex-col overflow-y-auto border-l border-zinc-800 bg-zinc-950 shadow-2xl">
        <div className="sticky top-0 z-10 flex items-start justify-between gap-3 border-b border-zinc-800 bg-zinc-950/95 px-5 py-4 backdrop-blur">
          <div className="min-w-0">
            {s && <SourceBadge source={s.source} />}
            <h2 className="mt-1 truncate text-base font-semibold text-zinc-50">{s?.title ?? "Session"}</h2>
            {s && (
              <div className="mt-0.5 text-xs text-zinc-500">
                {projectName(s.repoRoot ?? s.projectPath)}
                {s.gitBranch && ` · ${s.gitBranch}`} · {dateTime(s.startedAt)} · {s.model ?? "unknown model"}
              </div>
            )}
          </div>
          <Button variant="ghost" size="sm" onClick={onClose} aria-label="Close">
            <X size={16} />
          </Button>
        </div>

        {q.isLoading && <div className="px-5"><Loading /></div>}
        <div className="px-5"><ErrorNote error={q.error} /></div>

        {d && s && (
          <div className="space-y-6 px-5 py-5">
            <section className="rounded-xl border border-zinc-800 bg-zinc-900/60 p-4">
              <h3 className="mb-3 text-sm font-medium text-zinc-200">How did it go?</h3>
              <div className="space-y-3">
                <OutcomePicker value={outcome} onChange={setOutcome} />
                <div className="flex items-center gap-3">
                  <span className="text-xs text-zinc-500">Quality</span>
                  <Rating value={rating} onChange={setRating} />
                </div>
                <Field label="What would you do differently?">
                  <textarea className={inputClass} rows={2} value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="e.g. should have given it the failing test first" />
                </Field>
                <div className="flex flex-wrap items-center gap-1.5 text-xs">
                  <span className="text-zinc-400">Faster than doing it by hand?</span>
                  {(["faster", "same", "slower", "unsure"] as VsManual[]).map((v) => (
                    <button
                      key={v}
                      type="button"
                      onClick={() => setVsManual(vsManual === v ? null : v)}
                      className={`rounded-md px-2 py-0.5 capitalize ring-1 ring-inset ${vsManual === v ? "bg-zinc-700 text-zinc-50 ring-zinc-500" : "text-zinc-400 ring-zinc-700 hover:bg-zinc-800"}`}
                    >
                      {v === "same" ? "about the same" : v === "unsure" ? "not sure" : v}
                    </button>
                  ))}
                </div>
                {exps.data?.some((e) => e.status === "active") && (
                  <div className="space-y-1 text-sm">
                    <div className="text-xs text-zinc-400">Did this session apply an experiment?</div>
                    {exps.data
                      .filter((e) => e.status === "active")
                      .map((e) => (
                        <label key={e.id} className="flex items-center gap-2 text-zinc-300">
                          <input
                            type="checkbox"
                            className="accent-emerald-500"
                            checked={s.tags.includes(`exp-${e.id}`)}
                            onChange={(ev) => applied.mutate({ experimentId: e.id, on: ev.target.checked })}
                          />
                          {e.title}
                        </label>
                      ))}
                  </div>
                )}
                <Field label="Tags" hint="Comma separated">
                  <input className={inputClass} value={tags} onChange={(e) => setTags(e.target.value)} placeholder="refactor, flaky-tests" />
                </Field>
                <div className="flex items-center gap-3">
                  <Button variant="primary" busy={save.isPending} onClick={() => save.mutate()}>Save review</Button>
                  {save.isSuccess && <span className="text-xs text-emerald-400">Saved</span>}
                  <ErrorNote error={save.error} />
                </div>
              </div>
              {d.intent && (
                <div className="mt-4 border-t border-zinc-800 pt-3 text-xs text-zinc-400">
                  Journal intent: <span className="text-zinc-200">{d.intent.goal}</span> · expected {d.intent.expectedMinutes} min · confidence {d.intent.confidence}/5
                </div>
              )}
            </section>

            <section className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <Metric label="Active" value={duration(s.activeMs)} />
              <Metric label="Prompts" value={s.userTurns} />
              <Metric label="Interruptions" value={s.interruptions} />
              <Metric label="Tool errors" value={`${s.toolErrors}/${s.toolCalls}`} />
              <Metric label="Cost" value={money(s.costUsd, s.costIsEstimate)} hint={s.source !== "claude_code" ? "API-equivalent" : undefined} />
              <Metric label="Tokens" value={compact(d.inputTokens + d.outputTokens + d.cacheReadTokens + d.cacheWriteTokens)} />
              <Metric label="Waited on you" value={s.waitCount ? duration(s.waitMsTotal) : "—"} hint={s.waitCount ? `${s.waitCount} hand-backs` : undefined} />
              <Metric label="Lines" value={d.linesAdded != null ? `+${d.linesAdded} −${d.linesRemoved ?? 0}` : "—"} />
            </section>

            <section className="rounded-xl border border-zinc-800 bg-zinc-900/40 p-4">
              <div className="mb-3 flex items-center justify-between">
                <h3 className="text-sm font-medium text-zinc-200">
                  Flow score <span className="font-normal text-zinc-500">(5 = smooth)</span>
                  <Heuristic title="Derived from the transcript: clarity of the opening prompt, corrections and interruptions, and session size. Not a measure of quality." />
                </h3>
                {s.category && <span className="rounded-md bg-zinc-800 px-2 py-0.5 text-xs capitalize text-zinc-300">{s.category}</span>}
              </div>
              <div className="grid grid-cols-3 gap-3">
                <Score label="Clear ask" value={s.friction.requirements} hint="goal and done-criteria up front" />
                <Score label="Steering" value={s.friction.steering} hint={`${s.corrections} corrections, ${s.interruptions} interruptions`} />
                <Score label="Scope" value={s.friction.scope} hint={`${s.userTurns} prompts, ${duration(s.activeMs)}`} />
              </div>
              <div className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
                <Metric label="Reads / edits" value={`${s.reads} / ${s.edits}`} />
                <Metric label="Blind edits" value={s.editsWithoutRead} hint="edited without reading" />
                <Metric label="Re-edits" value={s.reEdits} hint="same file again" />
                <Metric label="Compactions · agents" value={`${s.compactions} · ${s.subagents}`} />
              </div>
              {Object.keys(d.toolCounts).length > 0 && (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {Object.entries(d.toolCounts)
                    .sort((a, b) => b[1] - a[1])
                    .slice(0, 14)
                    .map(([tool, n]) => (
                      <span key={tool} className="rounded-md bg-zinc-800/80 px-1.5 py-0.5 font-mono text-xs text-zinc-300">
                        {tool} <span className="text-zinc-500">{n}</span>
                      </span>
                    ))}
                </div>
              )}

            </section>

            {d.lastMessage && (
              <section>
                <h3 className="mb-2 text-sm font-medium text-zinc-200">Agent's last reply</h3>
                <div className="whitespace-pre-wrap break-words rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2 text-sm text-zinc-300">{d.lastMessage}</div>
              </section>
            )}

            {d.resumeCommand && <ResumeCommand command={d.resumeCommand} />}

            <section>
              <h3 className="mb-2 text-sm font-medium text-zinc-200">Commits ({d.commits.length})</h3>
              {d.commits.length === 0 ? (
                <p className="text-sm text-zinc-500">No commits by you in this repo during or within 30 minutes after this session.</p>
              ) : (
                <ul className="space-y-1.5">
                  {d.commits.map((c) => (
                    <li key={c.sha} className="flex items-start gap-2 text-sm">
                      <GitCommitHorizontal size={15} className="mt-0.5 shrink-0 text-zinc-500" />
                      <span className="font-mono text-xs text-zinc-500">{c.sha.slice(0, 7)}</span>
                      <span className="min-w-0 flex-1 truncate text-zinc-200">{c.subject}</span>
                      <span className="shrink-0 text-xs tabular-nums text-zinc-500">
                        <span className="text-emerald-400">+{c.insertions}</span> <span className="text-rose-400">−{c.deletions}</span>
                        {c.aiTrailer && <span className="ml-1.5 rounded-sm bg-zinc-800 px-1 text-zinc-400">AI</span>}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section>
              <h3 className="mb-2 text-sm font-medium text-zinc-200">Playbooks in effect</h3>
              {d.playbooks.length === 0 && <p className="text-sm text-zinc-500">No instruction files or prompt templates matched.</p>}
              <ul className="space-y-1">
                {d.playbooks.map((p) => (
                  <li key={p.versionId} className="flex items-center justify-between text-sm">
                    <span className="text-zinc-200">{p.name}</span>
                    <span className="text-xs text-zinc-500">
                      version of {dateTime(p.firstSeen)} · {p.link}
                      {p.link === "manual" && (
                        <button className="ml-2 text-zinc-400 hover:text-rose-300" onClick={() => togglePlaybook.mutate({ versionId: p.versionId, linked: false })}>
                          remove
                        </button>
                      )}
                    </span>
                  </li>
                ))}
              </ul>
              {templates.length > 0 && (
                <div className="mt-2">
                  <select
                    className={inputClass}
                    value=""
                    onChange={(e) => e.target.value && togglePlaybook.mutate({ versionId: Number(e.target.value), linked: true })}
                  >
                    <option value="">Mark a prompt template as used…</option>
                    {templates
                      .map((t) => ({ t, v: currentVersion(t)! }))
                      .filter(({ v }) => !linkedVersions.has(v.id))
                      .map(({ t, v }) => (
                        <option key={t.id} value={v.id}>{t.name}</option>
                      ))}
                  </select>
                </div>
              )}
            </section>

            <section>
              <h3 className="mb-2 text-sm font-medium text-zinc-200">Your prompts ({d.prompts.length})</h3>
              <ol className="space-y-2">
                {d.prompts.map((p) => (
                  <li key={p.idx} className="rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2">
                    <div className="mb-1 text-xs text-zinc-500">#{p.idx + 1} · {dateTime(p.ts)}</div>
                    <div className="whitespace-pre-wrap break-words text-sm text-zinc-300">{p.text}</div>
                  </li>
                ))}
              </ol>
            </section>

            <section className="flex items-center justify-between border-t border-zinc-800 pt-4 text-xs text-zinc-500">
              <span className="truncate font-mono">{d.filePath}</span>
              <Button size="sm" variant="ghost" onClick={() => revealItemInDir(d.filePath).catch(() => {})}>
                <FolderOpen size={14} /> Show file
              </Button>
            </section>
          </div>
        )}
      </div>
    </div>
  );
}

function Score({ label, value, hint }: { label: string; value: number; hint: string }) {
  const color = value >= 4 ? "text-emerald-300" : value >= 3 ? "text-amber-300" : "text-rose-300";
  // A symbol as well as a colour, so the state reads without colour vision.
  const mark = value >= 4 ? "✓" : value >= 3 ? "!" : "✕";
  return (
    <div>
      <div className="text-xs uppercase tracking-wide text-zinc-500">{label}</div>
      <div className={`text-lg font-semibold tabular-nums ${color}`}>
        <span aria-hidden className="mr-1 text-sm">{mark}</span>
        {value}
        <span className="text-xs text-zinc-500">/5</span>
      </div>
      <div className="text-xs text-zinc-500">{hint}</div>
    </div>
  );
}

function ResumeCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <section>
      <h3 className="mb-2 text-sm font-medium text-zinc-200">Pick it back up</h3>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-lg border border-zinc-800 bg-zinc-900/60 px-3 py-1.5 font-mono text-xs text-zinc-300" title={command}>{command}</code>
        <Button
          size="sm"
          onClick={() =>
            navigator.clipboard.writeText(command).then(
              () => setCopied(true),
              () => setCopied(false),
            )
          }
        >
          <Copy size={13} /> {copied ? "Copied" : "Copy"}
        </Button>
      </div>
    </section>
  );
}

function Metric({ label, value, hint }: { label: string; value: ReactNode; hint?: string }) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2">
      <div className="text-xs uppercase tracking-wide text-zinc-500">{label}</div>
      <div className="mt-0.5 text-sm font-semibold tabular-nums text-zinc-100">{value}</div>
      {hint && <div className="text-xs text-zinc-500">{hint}</div>}
    </div>
  );
}
