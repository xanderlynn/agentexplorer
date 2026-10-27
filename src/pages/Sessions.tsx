import { useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Download, Search } from "lucide-react";
import { api, OUTCOMES, type Outcome, type Source } from "../api";
import { Button, Empty, ErrorNote, Loading, OutcomeBadge, PageHeader, RANGE_OPTIONS, Segmented, SourceBadge, inputBase, inputClass } from "../components/ui";
import { OUTCOME_LABEL, SOURCE_LABEL, TASK_TYPES, dateTime, duration, money, projectName } from "../format";

type SortKey = "startedAt" | "activeMs" | "userTurns" | "interruptions" | "costUsd" | "commits";

export default function Sessions({ days, setDays, onOpen }: { days: number; setDays: (d: number) => void; onOpen: (id: string) => void }) {
  const [source, setSource] = useState<Source | "">("");
  const [outcome, setOutcome] = useState<Outcome | "unrated" | "">("");
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("");
  const [exported, setExported] = useState<string | null>(null);
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "startedAt", desc: true });

  const filter = { days, source: source || undefined, outcome: outcome || undefined, search: search || undefined, category: category || undefined };
  const exportCsv = useMutation({ mutationFn: () => api.exportSessions(filter), onSuccess: setExported });
  const q = useQuery({ queryKey: ["sessions", filter], queryFn: () => api.sessions(filter), placeholderData: (prev) => prev });

  const rows = useMemo(() => {
    const list = [...(q.data ?? [])];
    list.sort((a, b) => {
      const av = (a[sort.key] ?? -1) as number;
      const bv = (b[sort.key] ?? -1) as number;
      return sort.desc ? bv - av : av - bv;
    });
    return list;
  }, [q.data, sort]);

  const th = (key: SortKey, label: string, right = true) => (
    <th className={`cursor-pointer select-none pb-2 font-medium hover:text-zinc-300 ${right ? "text-right" : "text-left"}`} onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : true }))}>
      {label}
      {sort.key === key && (sort.desc ? " ↓" : " ↑")}
    </th>
  );

  return (
    <>
      <PageHeader title="Sessions" subtitle="Every tracked session. Open one to rate it; ratings are what make the insights work." actions={<Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />} />
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <div className="relative min-w-[16rem] flex-1">
          <Search size={14} className="absolute left-2.5 top-2.5 text-zinc-500" />
          <input className={`${inputClass} pl-8`} placeholder="Search every prompt, titles, projects, branches" value={search} onChange={(e) => setSearch(e.target.value)} />
        </div>
        <select className={inputBase} value={source} onChange={(e) => setSource(e.target.value as Source | "")}>
          <option value="">All tools</option>
          {(Object.keys(SOURCE_LABEL) as Source[]).map((s) => <option key={s} value={s}>{SOURCE_LABEL[s]}</option>)}
        </select>
        <select className={inputBase} value={outcome} onChange={(e) => setOutcome(e.target.value as Outcome | "unrated" | "")}>
          <option value="">Any outcome</option>
          <option value="unrated">Unrated</option>
          {OUTCOMES.map((o) => <option key={o} value={o}>{OUTCOME_LABEL[o]}</option>)}
        </select>
        <select className={inputBase} value={category} onChange={(e) => setCategory(e.target.value)}>
          <option value="">Any category</option>
          {TASK_TYPES.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
        <Button variant="ghost" busy={exportCsv.isPending} onClick={() => exportCsv.mutate()}>
          <Download size={14} /> Export CSV
        </Button>
      </div>

      {exported && (
        <p className="mb-3 text-xs text-zinc-500">
          Saved to <button className="font-mono text-zinc-300 underline-offset-2 hover:underline" onClick={() => revealItemInDir(exported).catch(() => {})}>{exported}</button>
        </p>
      )}
      <ErrorNote error={exportCsv.error} />
      {q.isLoading && <Loading />}
      <ErrorNote error={q.error} />
      {q.data && rows.length === 0 && <Empty title="No sessions match">Try a wider date range or clear the filters.</Empty>}
      {rows.length > 0 && (
        <div className="overflow-x-auto rounded-xl border border-zinc-800/80 bg-zinc-900/40">
          <table className="w-full text-sm">
            <thead className="border-b border-zinc-800 text-xs text-zinc-500">
              <tr className="[&>th]:px-3 [&>th]:pt-3">
                <th className="pb-2 text-left font-medium">Session</th>
                {th("startedAt", "Started", false)}
                {th("activeMs", "Active")}
                {th("userTurns", "Prompts")}
                {th("interruptions", "Interrupts")}
                {th("commits", "Commits")}
                {th("costUsd", "Cost")}
                <th className="pb-2 text-left font-medium">Outcome</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {rows.map((s) => (
                <tr key={s.id} className="cursor-pointer hover:bg-zinc-800/30 [&>td]:px-3 [&>td]:py-2" onClick={() => onOpen(s.id)}>
                  <td className="max-w-md">
                    <div className="truncate text-zinc-100">{s.title ?? "Untitled"}</div>
                    <div className="mt-0.5 flex items-center gap-2 text-xs text-zinc-500">
                      <SourceBadge source={s.source} />
                      <span className="truncate">· {projectName(s.repoRoot ?? s.projectPath)}{s.gitBranch ? ` · ${s.gitBranch}` : ""}{s.category ? ` · ${s.category}` : ""}</span>
                    </div>
                  </td>
                  <td className="whitespace-nowrap text-zinc-400">{dateTime(s.startedAt)}</td>
                  <td className="text-right tabular-nums text-zinc-300">{duration(s.activeMs)}</td>
                  <td className="text-right tabular-nums text-zinc-300">{s.userTurns}</td>
                  <td className={`text-right tabular-nums ${s.interruptions >= 2 ? "text-amber-300" : "text-zinc-300"}`}>{s.interruptions}</td>
                  <td className="text-right tabular-nums text-zinc-300">{s.commits || "—"}</td>
                  <td className="text-right tabular-nums text-zinc-400" title={s.source === "claude_code" ? "Recorded by Claude Code" : "API-equivalent estimate; Copilot is billed by subscription"}>
                    {money(s.costUsd, s.costIsEstimate)}
                  </td>
                  <td><OutcomeBadge outcome={s.outcome} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
