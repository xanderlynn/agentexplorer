import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, Sparkles, Trash2 } from "lucide-react";
import { api, type Retro as RetroT, type Suggestion } from "../api";
import { NewExperiment } from "./Experiments";
import type { Page } from "../App";
import { Markdown } from "../components/Markdown";
import { Button, Card, Empty, ErrorNote, Field, Loading, PageHeader, inputClass } from "../components/ui";
import { date, dateTime } from "../format";

export default function Retro({ go }: { go: (p: Page) => void }) {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const retros = useQuery({ queryKey: ["retros"], queryFn: api.retros });
  const [chosenDays, setChosenDays] = useState<number | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [reflection, setReflection] = useState("");
  const [adopting, setAdopting] = useState<{ retroId: number; s: Suggestion } | null>(null);
  const metrics = useQuery({ queryKey: ["metrics"], queryFn: api.metrics });
  const days = chosenDays ?? (Number(settings.data?.values["retro.days"]) || 7);
  const setDays = (d: number) => {
    setChosenDays(d);
    setPreview(null);
    api.setSetting("retro.days", String(d)).catch(() => {});
  };
  const [selected, setSelected] = useState<number | null>(null);

  const engine = settings.data?.values["retro.engine"] ?? "claude_cli";
  const ready = engine === "claude_cli" ? !!settings.data?.detectedClaudePath : !!settings.data?.hasApiKey;

  const generate = useMutation({
    mutationFn: () => api.generateRetro(engine, days, reflection.trim() || null),
    onSuccess: (r) => {
      setSelected(r.id);
      setPreview(null);
      qc.invalidateQueries({ queryKey: ["retros"] });
    },
  });
  const showPreview = useMutation({ mutationFn: () => api.previewRetro(days, reflection.trim() || null), onSuccess: setPreview });
  const del = useMutation({ mutationFn: (id: number) => api.deleteRetro(id), onSuccess: () => qc.invalidateQueries({ queryKey: ["retros"] }) });

  const list = retros.data ?? [];
  const current: RetroT | undefined = list.find((r) => r.id === selected) ?? list[0];

  return (
    <>
      <PageHeader title="Weekly retro" subtitle="Claude reviews your period: what worked, where execution leaked, and three experiments to try next. It follows up on last time's experiments." />
      <Card>
        <div className="flex flex-wrap items-end gap-3">
          <Field label="Period">
            <select className={`${inputClass} w-36`} value={days} onChange={(e) => setDays(Number(e.target.value))}>
              <option value={7}>Last 7 days</option>
              <option value={14}>Last 14 days</option>
              <option value={30}>Last 30 days</option>
            </select>
          </Field>
          <Button variant="primary" busy={generate.isPending} disabled={!ready} onClick={() => generate.mutate()}>
            <Sparkles size={15} /> {generate.isPending ? "Claude is reviewing…" : "Generate retro"}
          </Button>
          <Button variant="ghost" busy={showPreview.isPending} onClick={() => (preview ? setPreview(null) : showPreview.mutate())}>
            <Eye size={15} /> {preview ? "Hide" : "Preview"} what's sent
          </Button>
          <div className="text-xs text-zinc-500">
            Engine: {engine === "claude_cli" ? "Claude Code CLI (your subscription)" : `Anthropic API (${settings.data?.values["retro.apiModel"] || settings.data?.defaultApiModel})`} ·{" "}
            <button className="text-zinc-400 underline-offset-2 hover:underline" onClick={() => go("settings")}>change</button>
          </div>
        </div>
        {!ready && settings.data && (
          <p className="mt-3 text-sm text-amber-300">
            {engine === "claude_cli" ? "The claude CLI was not found. Install Claude Code or set its path in Settings." : "Add an Anthropic API key in Settings."}
          </p>
        )}
        <p className="mt-3 text-xs text-zinc-500">
          <Field label="Your reflection first (optional)" hint="What went well? What did you avoid or put off? Claude starts from your view.">
            <textarea className={inputClass} rows={3} value={reflection} onChange={(e) => setReflection(e.target.value)} placeholder="I kept steering the auth refactor instead of restarting it…" />
          </Field>
          <span className="mt-2 block" />
          Generating sends this period's metrics, session titles, opening prompts (up to 300 characters each), your notes and journal reflections to Claude. Nothing is sent until you click Generate.
        </p>
        <ErrorNote error={generate.error} />
        {preview && <pre className="mt-3 max-h-96 overflow-auto whitespace-pre-wrap rounded-lg border border-zinc-800 bg-zinc-950 p-3 font-mono text-xs text-zinc-400">{preview}</pre>}
      </Card>

      {adopting && metrics.data && (
        <div className="mt-6">
          <NewExperiment
            key={`${adopting.retroId}-${adopting.s.title}`}
            metrics={metrics.data}
            initial={{ ...adopting.s, retroId: adopting.retroId }}
            onDone={() => {
              setAdopting(null);
              go("experiments");
            }}
          />
        </div>
      )}
      {retros.isLoading && <Loading />}
      {retros.data && list.length === 0 && !generate.isPending && (
        <div className="mt-6"><Empty title="No retros yet">Rate this week's sessions first; the retro is much sharper with outcomes and notes.</Empty></div>
      )}
      {current && (
        <div className="mt-6 grid grid-cols-1 gap-4 xl:grid-cols-4">
          <Card className="xl:col-span-3">
            <div className="mb-4 flex items-center justify-between text-xs text-zinc-500">
              <span>{date(current.periodStart)} – {date(current.periodEnd)} · {current.engine === "claude_cli" ? "Claude Code CLI" : current.model}</span>
              <Button size="sm" variant="ghost" onClick={() => del.mutate(current.id)} aria-label="Delete retro"><Trash2 size={14} /></Button>
            </div>
            <div className="prose-retro">
              {current.reflection && (
                <blockquote className="mb-4">
                  <span className="text-xs uppercase tracking-wide text-zinc-500">Your reflection</span>
                  <br />
                  {current.reflection}
                </blockquote>
              )}
              <Markdown source={current.contentMd} />
              {current.suggestions.length > 0 && (
                <div className="mt-6 border-t border-zinc-800 pt-4">
                  <h3 className="mb-2 text-sm font-semibold text-zinc-100">Adopt an experiment</h3>
                  <ul className="space-y-2">
                    {current.suggestions.map((s, i) => (
                      <li key={i} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-zinc-800 px-3 py-2">
                        <span className="min-w-0 text-sm">
                          <span className="text-zinc-100">{s.title}</span>
                          <span className="block text-xs text-zinc-500">
                            {metrics.data?.find((m) => m.key === s.metric)?.label ?? s.metric}
                            {s.category && ` · ${s.category} tasks`}
                          </span>
                        </span>
                        <Button size="sm" onClick={() => setAdopting({ retroId: current.id, s })}>Adopt</Button>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          </Card>
          <Card title="History">
            <ul className="space-y-1">
              {list.map((r) => (
                <li key={r.id}>
                  <button
                    className={`w-full rounded-md px-2 py-1.5 text-left text-sm ${r.id === current.id ? "bg-zinc-800 text-zinc-100" : "text-zinc-400 hover:bg-zinc-800/50"}`}
                    onClick={() => setSelected(r.id)}
                  >
                    {dateTime(r.createdAt)}
                  </button>
                </li>
              ))}
            </ul>
          </Card>
        </div>
      )}
    </>
  );
}
