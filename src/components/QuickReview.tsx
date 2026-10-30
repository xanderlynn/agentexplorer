import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, OUTCOMES, type Outcome, type VsManual } from "../api";
import { OUTCOME_CLASS, OUTCOME_LABEL } from "../format";
import { cx } from "./ui";

const VS: { value: VsManual; label: string }[] = [
  { value: "faster", label: "Faster" },
  { value: "same", label: "About the same" },
  { value: "slower", label: "Slower" },
  { value: "unsure", label: "Not sure" },
];

/**
 * One-tap review: pick the outcome, optionally say whether it beat doing it
 * by hand. Saves on the first tap so it never costs more than a click.
 */
export function QuickReview({
  id,
  onDone,
  compact = false,
  suggested,
  evidence,
}: {
  id: string;
  onDone?: () => void;
  compact?: boolean;
  /** What its commits or PRs point to, offered as one click to confirm. */
  suggested?: Outcome | null;
  evidence?: string | null;
}) {
  const qc = useQueryClient();
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [vs, setVs] = useState<VsManual | null>(null);
  const save = useMutation({
    mutationFn: (v: { outcome: Outcome; vs: VsManual | null }) => api.quickReview(id, v.outcome, v.vs),
    onSuccess: () => qc.invalidateQueries(),
  });

  const pick = (o: Outcome) => {
    setOutcome(o);
    save.mutate({ outcome: o, vs });
  };
  const pickVs = (v: VsManual) => {
    setVs(v);
    if (outcome) save.mutate({ outcome, vs: v }, { onSuccess: () => onDone?.() });
  };

  return (
    <div className={cx("space-y-2", compact && "space-y-1.5")}>
      {suggested && !outcome && (
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="text-zinc-400">
            Looks {OUTCOME_LABEL[suggested].toLowerCase()}
            {evidence && <span className="text-zinc-500">: {evidence}</span>}
          </span>
          <button type="button" onClick={() => pick(suggested)} className={cx("rounded-md px-2 py-0.5 font-medium ring-1 ring-inset", OUTCOME_CLASS[suggested])}>
            Confirm
          </button>
        </div>
      )}
      <div className="flex flex-wrap gap-1.5" role="group" aria-label="Outcome">
        {OUTCOMES.map((o) => (
          <button
            key={o}
            type="button"
            onClick={() => pick(o)}
            className={cx(
              "rounded-md px-2.5 py-1 text-xs font-medium ring-1 ring-inset transition-colors",
              outcome === o ? OUTCOME_CLASS[o] : "text-zinc-300 ring-zinc-700 hover:bg-zinc-800",
            )}
          >
            {OUTCOME_LABEL[o]}
          </button>
        ))}
      </div>
      {outcome && (
        <div className="flex flex-wrap items-center gap-1.5 text-xs" role="group" aria-label="Compared with doing it by hand">
          <span className="text-zinc-400">Faster than by hand?</span>
          {VS.map((v) => (
            <button
              key={v.value}
              type="button"
              onClick={() => pickVs(v.value)}
              className={cx(
                "rounded-md px-2 py-0.5 ring-1 ring-inset",
                vs === v.value ? "bg-zinc-700 text-zinc-50 ring-zinc-500" : "text-zinc-400 ring-zinc-700 hover:bg-zinc-800",
              )}
            >
              {v.label}
            </button>
          ))}
          {onDone && !vs && (
            <button type="button" className="ml-1 text-zinc-500 hover:text-zinc-300" onClick={onDone}>
              skip
            </button>
          )}
        </div>
      )}
      {save.error && <div className="text-xs text-rose-400">{String(save.error)}</div>}
    </div>
  );
}
