import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bell, CheckCircle2, CircleDashed, Shield, X } from "lucide-react";
import { api } from "../api";
import { Button, Card, ErrorNote, SourceBadge, cx } from "./ui";

const KEY = "setupDone";

/**
 * A first-run checklist on Today: which tools' logs Tilly found, a test
 * notification, Claude Code's permission-prompt hook, and whether to keep
 * transcript copies. Gone once dismissed.
 */
export function FirstRun() {
  const qc = useQueryClient();
  const [done, setDone] = useState(() => {
    try {
      return localStorage.getItem(KEY) === "1";
    } catch {
      return true;
    }
  });
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources, enabled: !done });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings, enabled: !done });
  const hook = useQuery({ queryKey: ["claudeHook"], queryFn: api.claudeHook, enabled: !done });
  const notify = useMutation({ mutationFn: api.testNotification });
  const addHook = useMutation({ mutationFn: () => api.setClaudeHook(true), onSuccess: (h) => qc.setQueryData(["claudeHook"], h) });
  const archive = useMutation({
    mutationFn: (on: boolean) => api.setSetting("archive.enabled", String(on)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });
  if (done) return null;
  const finish = () => {
    try {
      localStorage.setItem(KEY, "1");
    } catch {
      // Without storage it shows again next time; harmless.
    }
    setDone(true);
  };
  const claude = sources.data?.find((s) => s.source === "claude_code");
  const keeping = settings.data?.values["archive.enabled"] !== "false";
  const Step = ({ ok, children }: { ok: boolean; children: React.ReactNode }) => (
    <li className="flex items-start gap-3">
      {ok ? <CheckCircle2 size={16} className="mt-0.5 shrink-0 text-emerald-400" /> : <CircleDashed size={16} className="mt-0.5 shrink-0 text-zinc-500" />}
      <div className="min-w-0 flex-1 text-sm text-zinc-300">{children}</div>
    </li>
  );
  return (
    <Card
      title="Set up Tilly"
      className="mb-4"
      action={
        <button className="text-zinc-500 hover:text-zinc-200" aria-label="Done with setup" onClick={finish}>
          <X size={16} />
        </button>
      }
    >
      <ol className="space-y-3">
        <Step ok={!!sources.data?.some((s) => s.found)}>
          <div>Your agents&apos; logs</div>
          <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-xs">
            {sources.data?.map((s) => (
              <span key={s.source} className={cx("inline-flex items-center gap-1.5", !s.found && "opacity-50")}>
                <SourceBadge source={s.source} /> {s.found ? `${s.sessions} sessions` : "not found"}
              </span>
            ))}
          </div>
          <div className="mt-1 text-xs text-zinc-500">Missing one that's in a custom folder? Add it in Settings › Sources.</div>
        </Step>
        <Step ok={notify.isSuccess}>
          <div className="flex flex-wrap items-center gap-2">
            Notifications, so you hear when an agent waits on you
            <Button size="sm" busy={notify.isPending} onClick={() => notify.mutate()}>
              <Bell size={13} /> Send a test
            </Button>
          </div>
          <ErrorNote error={notify.error} />
        </Step>
        {claude?.found && (
          <Step ok={!!hook.data?.installed}>
            <div className="flex flex-wrap items-center gap-2">
              Exact Claude Code permission prompts, Bash included
              {!hook.data?.installed && (
                <Button size="sm" busy={addHook.isPending} onClick={() => addHook.mutate()}>
                  <Shield size={13} /> Add Tilly&apos;s hook
                </Button>
              )}
            </div>
            <div className="mt-1 text-xs text-zinc-500">One entry in Claude Code&apos;s settings.json, passing Tilly the session and tool. Remove it any time in Settings.</div>
            <ErrorNote error={addHook.error} />
          </Step>
        )}
        <Step ok={settings.data?.values["archive.enabled"] != null}>
          <div className="flex flex-wrap items-center gap-2">
            Keep copies of transcripts, so history outlives the tools&apos; own cleanup?
            <Button size="sm" variant={keeping ? "primary" : "secondary"} onClick={() => archive.mutate(true)}>
              Keep
            </Button>
            <Button size="sm" variant={!keeping ? "primary" : "secondary"} onClick={() => archive.mutate(false)}>
              Don&apos;t keep
            </Button>
          </div>
          <div className="mt-1 text-xs text-zinc-500">Copies stay on this computer, in Tilly&apos;s data folder.</div>
        </Step>
      </ol>
      <div className="mt-4 flex justify-end">
        <Button size="sm" variant="primary" onClick={finish}>
          Done
        </Button>
      </div>
    </Card>
  );
}
