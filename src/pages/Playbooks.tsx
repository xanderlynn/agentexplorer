import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, Plus, Trash2 } from "lucide-react";
import { api, type Playbook, type PlaybookVersion } from "../api";
import { Button, Card, Empty, ErrorNote, Field, Loading, PageHeader, Rate, inputClass } from "../components/ui";
import { date, fixed, pct, projectName } from "../format";
import { MIN_N } from "../stats";

const KIND_LABEL: Record<Playbook["kind"], string> = {
  claude_md: "CLAUDE.md",
  agents_md: "AGENTS.md",
  copilot_instructions: "Copilot instructions",
  prompt: "Prompt template",
};

export default function Playbooks() {
  const q = useQuery({ queryKey: ["playbooks"], queryFn: api.playbooks });
  const [creating, setCreating] = useState(false);
  const files = q.data?.filter((p) => p.kind !== "prompt") ?? [];
  const prompts = q.data?.filter((p) => p.kind === "prompt") ?? [];

  return (
    <>
      <PageHeader
        title="Playbooks"
        subtitle="The instructions you give agents, versioned. Each session is linked to the version in effect when it ran, so you can see whether an edit helped."
        actions={<Button variant="primary" onClick={() => setCreating(true)}><Plus size={15} /> Prompt template</Button>}
      />
      {creating && <TemplateEditor onDone={() => setCreating(false)} />}
      {q.isLoading && <Loading />}
      <ErrorNote error={q.error} />

      <h2 className="mb-3 mt-2 text-sm font-medium text-zinc-300">Instruction files</h2>
      {q.data && files.length === 0 ? (
        <Empty title="No instruction files found">
          Add a CLAUDE.md, AGENTS.md or .github/copilot-instructions.md to a project you work in. Past versions are read from git history automatically.
        </Empty>
      ) : (
        <div className="space-y-3">{files.map((p) => <PlaybookCard key={p.id} p={p} />)}</div>
      )}

      <h2 className="mb-3 mt-8 text-sm font-medium text-zinc-300">Prompt templates</h2>
      {q.data && prompts.length === 0 ? (
        <Empty title="No prompt templates yet">
          Save prompts you reuse ("review this diff for…", "write tests for…"). Sessions whose first prompt contains the template's opening are linked automatically, or link them by hand from a session.
        </Empty>
      ) : (
        <div className="space-y-3">{prompts.map((p) => <PlaybookCard key={p.id} p={p} />)}</div>
      )}
    </>
  );
}

function PlaybookCard({ p }: { p: Playbook }) {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const del = useMutation({ mutationFn: () => api.deletePlaybook(p.id), onSuccess: () => qc.invalidateQueries() });
  const used = p.versions.filter((v) => v.sessions > 0);
  const best = used.filter((v) => v.rated >= 3 && v.successRate != null).sort((a, b) => b.successRate! - a.successRate!)[0];

  return (
    <Card>
      <div className="flex items-start justify-between gap-3">
        <button className="flex min-w-0 items-start gap-2 text-left" onClick={() => setOpen(!open)}>
          {open ? <ChevronDown size={16} className="mt-0.5 text-zinc-500" /> : <ChevronRight size={16} className="mt-0.5 text-zinc-500" />}
          <div className="min-w-0">
            <div className="text-sm font-medium text-zinc-100">{p.name}</div>
            <div className="mt-0.5 text-xs text-zinc-500">
              {KIND_LABEL[p.kind]} · {p.versions.length} version{p.versions.length === 1 ? "" : "s"} · {used.reduce((n, v) => n + v.sessions, 0)} sessions
              {p.projectRoot ? ` · ${projectName(p.projectRoot)}` : p.kind !== "prompt" ? " · all projects" : ""}
              {best && ` · best version ${pct(best.successRate)} success`}
            </div>
          </div>
        </button>
        {p.kind === "prompt" && (
          <div className="flex gap-1">
            <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>Edit</Button>
            <Button size="sm" variant="ghost" onClick={() => del.mutate()} aria-label="Delete"><Trash2 size={14} /></Button>
          </div>
        )}
      </div>
      {editing && <TemplateEditor existing={p} onDone={() => setEditing(false)} />}
      {open && (
        <div className="mt-3 overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-zinc-500">
                <th className="pb-2 font-medium">Version</th>
                <th className="pb-2 text-right font-medium">Sessions</th>
                <th className="pb-2 text-right font-medium">Success</th>
                <th className="pb-2 text-right font-medium">Prompts/session</th>
                <th className="pb-2 text-right font-medium">Interrupts</th>
                <th className="pb-2 text-right font-medium">Tool errors</th>
                <th className="pb-2 text-right font-medium">Commits/session</th>
                <th className="pb-2 text-right font-medium">Reworked</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {p.versions.map((v, i) => <VersionRow key={v.id} v={v} prev={p.versions[i + 1]} />)}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

/** Change in success rate from the previous version, when both have enough ratings. */
function SuccessDelta({ v, prev }: { v: PlaybookVersion; prev?: PlaybookVersion }) {
  if (!prev || v.successRate == null || prev.successRate == null || v.rated < MIN_N || prev.rated < MIN_N) return null;
  const d = v.successRate - prev.successRate;
  if (Math.abs(d) < 0.05) return <span className="ml-1 text-xs text-zinc-500">≈</span>;
  return <span className={`ml-1 text-xs ${d > 0 ? "text-emerald-400" : "text-rose-400"}`}>{d > 0 ? "▲" : "▼"}{Math.round(Math.abs(d) * 100)}</span>;
}

function VersionRow({ v, prev }: { v: PlaybookVersion; prev?: PlaybookVersion }) {
  const [show, setShow] = useState(false);
  return (
    <>
      <tr className="cursor-pointer hover:bg-zinc-800/30" onClick={() => setShow(!show)}>
        <td className="py-1.5 text-zinc-300">
          {date(v.firstSeen)} {v.gitSha && <span className="font-mono text-xs text-zinc-500">{v.gitSha.slice(0, 7)}</span>}
        </td>
        <td className="py-1.5 text-right tabular-nums text-zinc-300">{v.sessions}</td>
        <td className="py-1.5 text-right tabular-nums text-zinc-300">
          <span className="inline-flex items-center justify-end">
            <Rate rate={v.successRate} n={v.rated} bar={false} />
            <SuccessDelta v={v} prev={prev} />
          </span>
        </td>
        <td className="py-1.5 text-right tabular-nums text-zinc-300">{fixed(v.avgTurns)}</td>
        <td className="py-1.5 text-right tabular-nums text-zinc-300">{fixed(v.avgInterruptions)}</td>
        <td className="py-1.5 text-right tabular-nums text-zinc-300">{pct(v.toolErrorRate, 1)}</td>
        <td className="py-1.5 text-right tabular-nums text-zinc-300">{fixed(v.commitsPerSession)}</td>
        <td className="py-1.5 text-right tabular-nums text-zinc-300">{pct(v.reworkRate)}</td>
      </tr>
      {show && (
        <tr>
          <td colSpan={8} className="pb-3">
            <pre className="max-h-80 overflow-auto whitespace-pre-wrap rounded-lg border border-zinc-800 bg-zinc-950 p-3 font-mono text-xs text-zinc-300">{v.content}</pre>
          </td>
        </tr>
      )}
    </>
  );
}

function TemplateEditor({ existing, onDone }: { existing?: Playbook; onDone: () => void }) {
  const qc = useQueryClient();
  const [name, setName] = useState(existing?.name ?? "");
  const [content, setContent] = useState((existing && existing.versions[0]?.content) ?? "");
  const save = useMutation({
    mutationFn: () => (existing ? api.updatePromptTemplate(existing.id, name, content) : api.createPromptTemplate(name, content).then(() => undefined)),
    onSuccess: () => {
      qc.invalidateQueries();
      onDone();
    },
  });
  return (
    <Card className="my-4">
      <div className="space-y-3">
        <Field label="Name"><input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} placeholder="Code review" /></Field>
        <Field label="Template" hint="Editing creates a new version; old sessions stay linked to the version they used.">
          <textarea className={`${inputClass} font-mono`} rows={8} value={content} onChange={(e) => setContent(e.target.value)} />
        </Field>
        <div className="flex gap-2">
          <Button variant="primary" busy={save.isPending} disabled={!name.trim() || !content.trim()} onClick={() => save.mutate()}>Save</Button>
          <Button variant="ghost" onClick={onDone}>Cancel</Button>
        </div>
        <ErrorNote error={save.error} />
      </div>
    </Card>
  );
}
