import { useMutation } from "@tanstack/react-query";
import { SquareArrowOutUpRight } from "lucide-react";
import { api } from "../api";
import { Button, cx } from "./ui";

/**
 * Brings forward where the session runs: the Claude or Copilot app, VS Code,
 * or the exact Terminal or iTerm tab of a command-line agent. Says what it
 * did, or how to pick the session up if it couldn't find it.
 */
export function GoToAgent({ id, className }: { id: string; className?: string }) {
  const go = useMutation({ mutationFn: () => api.goToAgent(id) });
  const note = go.isError ? String((go.error as { message?: string })?.message ?? go.error) : go.data;
  return (
    <span className={cx("inline-flex min-w-0 flex-col items-end gap-1", className)}>
      <Button
        size="sm"
        variant="secondary"
        busy={go.isPending}
        onClick={(e) => {
          e.stopPropagation();
          go.mutate();
        }}
      >
        <SquareArrowOutUpRight size={13} /> Go to agent
      </Button>
      {note && <span className={cx("max-w-xs text-right text-[11px]", go.isError ? "text-amber-300" : "text-zinc-500")}>{note}</span>}
    </span>
  );
}
