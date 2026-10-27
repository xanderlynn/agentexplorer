import { AlertTriangle, CheckCircle2, ChevronRight, Info } from "lucide-react";
import type { Insight } from "../api";

export function InsightItem({ insight, onOpen }: { insight: Insight; onOpen?: () => void }) {
  const Icon = insight.kind === "warning" ? AlertTriangle : insight.kind === "positive" ? CheckCircle2 : Info;
  const color = insight.kind === "warning" ? "text-amber-400" : insight.kind === "positive" ? "text-emerald-400" : "text-sky-400";
  const label = insight.kind === "warning" ? "Warning" : insight.kind === "positive" ? "Going well" : "Note";
  const body = (
    <>
      <Icon size={18} className={`mt-0.5 shrink-0 ${color}`} aria-label={label} />
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium text-zinc-100">{insight.title}</div>
        <div className="mt-0.5 text-sm text-zinc-400">{insight.detail}</div>
        <div className="mt-1 text-xs text-zinc-500">{insight.evidence}</div>
      </div>
      {onOpen && <ChevronRight size={16} className="mt-1 shrink-0 text-zinc-500" aria-hidden />}
    </>
  );
  return (
    <li>
      {onOpen ? (
        <button onClick={onOpen} className="flex w-full gap-3 rounded-lg p-2 text-left transition-colors hover:bg-zinc-800/40" title={`Open the ${insight.sessionIds.length} sessions behind this`}>
          {body}
        </button>
      ) : (
        <div className="flex gap-3 p-2">{body}</div>
      )}
    </li>
  );
}
