import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { CheckCircle2, Download, FolderOpen, RefreshCw, XCircle } from "lucide-react";
import { api, useReleases, type IngestReport } from "../api";
import { AppearanceSettings } from "../components/Appearance";
import { StatusProfiles } from "../components/StatusProfiles";
import { Updates, hasUpdate } from "../components/Updates";
import { CURRENT_VERSION } from "../components/WhatsNew";
import { Button, Card, ErrorNote, Field, Loading, PageHeader, Segmented, SourceBadge, cx, inputClass } from "../components/ui";

export default function Settings() {
  const qc = useQueryClient();
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const [report, setReport] = useState<IngestReport | null>(null);
  const [key, setKey] = useState("");
  const [updatesOpen, setUpdatesOpen] = useState(false);

  // Everything but the release list, which comes from GitHub on its own schedule.
  const invalidate = () => qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "releases" });
  const rescan = useMutation({ mutationFn: (force: boolean) => api.rescan(force), onSuccess: (r) => { setReport(r); invalidate(); } });
  const toggleSource = useMutation({ mutationFn: (v: { source: Parameters<typeof api.setSourceEnabled>[0]; enabled: boolean }) => api.setSourceEnabled(v.source, v.enabled), onSuccess: invalidate });
  const set = useMutation({ mutationFn: (v: { key: string; value: string }) => api.setSetting(v.key, v.value), onSuccess: invalidate });
  const saveKey = useMutation({ mutationFn: () => api.setApiKey(key), onSuccess: () => { setKey(""); invalidate(); } });
  const clearKey = useMutation({ mutationFn: api.clearApiKey, onSuccess: invalidate });

  const v = settings.data?.values ?? {};
  const bool = (k: string, d = true) => (v[k] == null ? d : v[k] === "true");
  const engine = v["retro.engine"] ?? "claude_cli";
  const releases = useReleases(!!settings.data && bool("updates.autoCheck"));
  const openUpdates = () => {
    setUpdatesOpen(true);
    if (!releases.data) releases.refetch();
  };

  return (
    <>
      <PageHeader title="Settings" />
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <AppearanceSettings />
        <StatusProfiles />
        <Card
          title="Sources"
          action={
            <div className="flex gap-2">
              <Button size="sm" busy={rescan.isPending && rescan.variables === false} onClick={() => rescan.mutate(false)}><RefreshCw size={13} /> Scan now</Button>
              <Button size="sm" variant="ghost" busy={rescan.isPending && rescan.variables === true} onClick={() => rescan.mutate(true)}>Re-parse everything</Button>
            </div>
          }
        >
          {sources.isLoading && <Loading />}
          <ul className="space-y-3">
            {sources.data?.map((s) => (
              <li key={s.source} className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <SourceBadge source={s.source} />
                    {s.found ? <CheckCircle2 size={14} className="text-emerald-400" /> : <XCircle size={14} className="text-zinc-500" />}
                    <span className="text-xs text-zinc-500">{s.sessions} sessions</span>
                  </div>
                  <div className="mt-0.5 space-y-0.5">
                    {s.paths.length ? s.paths.map((p) => <div key={p} className="truncate font-mono text-xs text-zinc-500">{p}</div>) : <div className="text-xs text-zinc-500">Not found on this computer</div>}
                  </div>
                  <ExtraDirs
                    source={s.source}
                    json={v["sources.extraDirs"]}
                    onSave={(value) => set.mutate({ key: "sources.extraDirs", value }, { onSuccess: () => rescan.mutate(false) })}
                  />
                </div>
                <label className="flex shrink-0 items-center gap-2 text-xs text-zinc-400">
                  <input type="checkbox" className="accent-emerald-500" checked={s.enabled} onChange={(e) => toggleSource.mutate({ source: s.source, enabled: e.target.checked })} />
                  Track
                </label>
              </li>
            ))}
          </ul>
          {report && (
            <p className="mt-4 text-xs text-zinc-500">
              Scanned {report.scanned} files, updated {report.updated}{report.failed ? `, ${report.failed} failed` : ""} in {report.durationMs} ms.
              {report.errors.slice(0, 3).map((e) => <span key={e} className="block truncate text-rose-400">{e}</span>)}
            </p>
          )}
          <ErrorNote error={rescan.error} />
        </Card>

        <Card title="Weekly retro">
          <div className="space-y-4">
            <Field label="Engine">
              <select className={inputClass} value={engine} onChange={(e) => set.mutate({ key: "retro.engine", value: e.target.value })}>
                <option value="claude_cli">Claude Code CLI: uses your Claude subscription, no key needed</option>
                <option value="anthropic_api">Anthropic API: pay per use with your API key</option>
              </select>
            </Field>
            {engine === "claude_cli" ? (
              <>
                <p className="text-xs text-zinc-500">
                  Detected: <span className="font-mono">{settings.data?.detectedClaudePath ?? "not found"}</span>
                </p>
                <Field label="claude path override" hint="Leave empty to auto-detect.">
                  <SettingInput settingKey="retro.claudePath" values={v} ready={!!settings.data} onSave={(key, value) => set.mutate({ key, value })} placeholder="/Users/you/.local/bin/claude" />
                </Field>
                <Field label="Model" hint="Optional; passed as --model. Empty uses the CLI's default.">
                  <SettingInput settingKey="retro.cliModel" values={v} ready={!!settings.data} onSave={(key, value) => set.mutate({ key, value })} placeholder="opus" />
                </Field>
              </>
            ) : (
              <>
                <Field label="Model">
                  <SettingInput settingKey="retro.apiModel" values={v} ready={!!settings.data} onSave={(key, value) => set.mutate({ key, value })} placeholder={settings.data?.defaultApiModel} />
                </Field>
                <Field label="API key" hint="Stored in your OS keychain (macOS Keychain / Windows Credential Manager), never in the database.">
                  {settings.data?.hasApiKey ? (
                    <div className="flex items-center gap-3">
                      <span className="text-sm text-emerald-400">Key saved</span>
                      <Button size="sm" variant="danger" busy={clearKey.isPending} onClick={() => clearKey.mutate()}>Remove</Button>
                    </div>
                  ) : (
                    <div className="flex gap-2">
                      <input type="password" autoComplete="off" className={inputClass} value={key} onChange={(e) => setKey(e.target.value)} placeholder="sk-ant-…" />
                      <Button busy={saveKey.isPending} disabled={!key.trim()} onClick={() => saveKey.mutate()}>Save</Button>
                    </div>
                  )}
                </Field>
                {settings.data?.keychainError && <p className="text-xs text-rose-400">Keychain: {settings.data.keychainError}</p>}
                <ErrorNote error={saveKey.error} />
              </>
            )}
          </div>
        </Card>

        <Card title="Behaviour">
          <div className="space-y-3">
            <Field label="Smart-zone limit (tokens)" hint="Sessions whose context grows past this are flagged. Models follow instructions less reliably long before their window is full; 100k is a sensible default.">
              <SettingInput settingKey="context.smartZoneTokens" values={v} ready={!!settings.data} onSave={(key, value) => set.mutate({ key, value })} placeholder="100000" />
            </Field>
            <Field label="Monthly AI budget (USD)" hint="Optional. Shown against your month-to-date pace on the Usage page.">
              <SettingInput settingKey="budget.monthly" values={v} ready={!!settings.data} onSave={(key, value) => set.mutate({ key, value })} placeholder="e.g. 200" />
            </Field>
            <label className="flex items-start gap-3 text-sm text-zinc-300">
              <input type="checkbox" className="mt-1 accent-emerald-500" checked={bool("notify.waiting")} onChange={(e) => set.mutate({ key: "notify.waiting", value: String(e.target.checked) })} />
              <span>
                Notify me when an agent starts waiting on me
                <span className="block text-xs text-zinc-500">A desktop notification when a session finishes its turn or asks for permission.</span>
              </span>
            </label>
            <label className="flex items-start gap-3 text-sm text-zinc-300">
              <input type="checkbox" className="mt-1 accent-emerald-500" checked={bool("archive.enabled")} onChange={(e) => set.mutate({ key: "archive.enabled", value: String(e.target.checked) })} />
              <span>
                Keep a copy of raw transcripts
                <span className="block text-xs text-zinc-500">Claude Code deletes transcripts after 30 days by default. A copy keeps your full history for future analysis.</span>
              </span>
            </label>
          </div>
        </Card>

        <Card title="Updates">
          <div className="flex items-center justify-between gap-3">
            <p className="text-sm text-zinc-400">
              Version {CURRENT_VERSION}
              {hasUpdate(releases.data) && <span className="ml-2 text-emerald-300">A newer version is available.</span>}
            </p>
            <Button size="sm" variant="secondary" onClick={openUpdates}>
              <Download size={14} /> Choose version
            </Button>
          </div>
          <label className="mt-3 flex items-start gap-3 text-sm text-zinc-300">
            <input type="checkbox" className="mt-1 accent-emerald-500" checked={bool("updates.autoCheck")} onChange={(e) => set.mutate({ key: "updates.autoCheck", value: String(e.target.checked) })} />
            <span>
              Check for updates automatically
              <span className="block text-xs text-zinc-500">On launch and once a day. Only GitHub's public release list is fetched; nothing about you is sent.</span>
            </span>
          </label>
        </Card>
        <Card title="How you pay for agents">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <span className="text-sm text-zinc-300">Plan</span>
            <Segmented
              options={[
                { value: "api", label: "Pay per use (API)" },
                { value: "subscription", label: "Subscription" },
              ]}
              value={v["billing.plan"] ?? "api"}
              onChange={(value) => set.mutate({ key: "billing.plan", value })}
            />
          </div>
          <p className="mt-2 text-xs text-zinc-500">
            On a subscription (Claude Pro or Max, a Copilot plan) you don't pay per token, so dollar amounts show as ≈, what the same work would cost at API prices: useful to compare, not a bill.
          </p>
        </Card>
        <ClaudeHookCard />
        <Card title="Workstreams">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <span className="text-sm text-zinc-300">Sync imported items</span>
            <Segmented
              options={[
                { value: "0", label: "Off" },
                { value: "5", label: "5 min" },
                { value: "15", label: "15 min" },
                { value: "60", label: "Hourly" },
              ]}
              value={v["workstreams.syncMinutes"] ?? "15"}
              onChange={(value) => set.mutate({ key: "workstreams.syncMinutes", value })}
            />
          </div>
          <p className="mt-2 text-xs text-zinc-500">
            While Tilly runs, workstreams imported from GitHub or Azure DevOps are fetched again this often, and when you come back to the window. Fetching uses your own gh or az sign-in.
            Opening one, or Sync, always fetches it.
          </p>
        </Card>
        {updatesOpen && <Updates releases={releases.data} loading={releases.isFetching} error={releases.error} onRefresh={() => releases.refetch()} onClose={() => setUpdatesOpen(false)} />}

        <Card title="Data">
          <p className="text-sm text-zinc-400">Everything lives in one SQLite database on this computer.</p>
          <div className="mt-2 flex items-center justify-between gap-3">
            <span className="truncate font-mono text-xs text-zinc-500">{settings.data?.dataDir}</span>
            <Button size="sm" variant="ghost" onClick={() => settings.data && revealItemInDir(settings.data.dbPath).catch(() => {})}>
              <FolderOpen size={14} /> Show
            </Button>
          </div>
          <div className="mt-2 flex items-center justify-between gap-3">
            <span className="text-xs text-zinc-500">The log records background errors and failed actions; include it when you report a problem.</span>
            <Button size="sm" variant="ghost" onClick={() => settings.data && revealItemInDir(settings.data.logPath).catch(() => {})}>
              <FolderOpen size={14} /> Show log
            </Button>
          </div>
        </Card>
        <Licenses />
      </div>
      <ErrorNote error={set.error ?? toggleSource.error} />
    </>
  );
}

// A text setting saved on blur. It mounts only once settings have loaded, so
// its initial value is the stored one, and an unchanged value is not re-saved.
function SettingInput({
  settingKey,
  values,
  ready,
  onSave,
  placeholder,
}: {
  settingKey: string;
  values: Record<string, string>;
  ready: boolean;
  onSave: (key: string, value: string) => void;
  placeholder?: string;
}) {
  const stored = values[settingKey] ?? "";
  if (!ready) return <input className={inputClass} disabled placeholder={placeholder} />;
  return (
    <input
      key={stored}
      className={inputClass}
      defaultValue={stored}
      placeholder={placeholder}
      onBlur={(e) => {
        const value = e.target.value.trim();
        if (value !== stored) onSave(settingKey, value);
      }}
    />
  );
}

// MIT, Apache-2.0, BSD and ISC require shipping these notices with the app.
// The file is generated at build time by scripts/notices.mjs.
function Licenses() {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () =>
    fetch("/third-party-notices.txt")
      .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then(setText)
      .catch((e) => setError(String(e)));
  return (
    <Card title="About and licenses" className="xl:col-span-2">
      <p className="text-sm text-zinc-400">Tilly is MIT licensed. It is built on open-source software under permissive licenses (MIT, Apache-2.0, BSD, ISC and similar).</p>
      <div className="mt-3">
        {text == null ? (
          <Button size="sm" onClick={load}>Show third-party notices</Button>
        ) : (
          <pre className="max-h-96 overflow-auto whitespace-pre-wrap rounded-lg border border-zinc-800 bg-zinc-950 p-3 font-mono text-xs text-zinc-400">{text}</pre>
        )}
        {error && <p className="mt-2 text-xs text-rose-400">Could not load notices: {error}</p>}
      </div>
    </Card>
  );
}

/**
 * Claude Code doesn't log permission prompts; with this hook it tells Tilly
 * when one appears, for Claude Code in a terminal and in the Claude app.
 */
function ClaudeHookCard() {
  const qc = useQueryClient();
  const hook = useQuery({ queryKey: ["claudeHook"], queryFn: api.claudeHook });
  const set = useMutation({ mutationFn: (on: boolean) => api.setClaudeHook(on), onSuccess: (h) => qc.setQueryData(["claudeHook"], h) });
  const on = hook.data?.installed ?? false;
  return (
    <Card title="Claude Code permission prompts">
      <p className="text-sm text-zinc-400">
        Claude Code doesn't record its permission prompts, so Tilly can only guess that one is waiting (Might need approval), and can't see Bash prompts at all. Add Tilly&apos;s
        hook to Claude Code and each prompt shows as <span className="text-zinc-200">Needs permission</span> the moment it appears, in the terminal and in the Claude app.
      </p>
      <p className="mt-2 text-xs text-zinc-500">
        Adds one Notification hook to {hook.data?.settingsPath ?? "Claude Code's settings"} (the file before is kept as settings.json.tilly-backup). The hook passes Tilly the
        session and the tool it asks for, nothing else. Remove it here any time.
      </p>
      <div className="mt-3 flex items-center gap-3">
        <Button size="sm" variant={on ? "secondary" : "primary"} busy={set.isPending} onClick={() => set.mutate(!on)}>
          {on ? "Remove the hook" : "Add the hook"}
        </Button>
        <span className={cx("text-xs", on ? "text-emerald-300" : "text-zinc-500")}>{hook.isLoading ? "Checking…" : on ? "Installed" : "Not installed"}</span>
      </div>
      <ErrorNote error={set.error ?? hook.error} />
    </Card>
  );
}

/**
 * More folders to read a tool's logs from, beyond its usual one: another
 * config folder (such as a CLAUDE_CONFIG_DIR a Finder-launched Tilly can't
 * see) or its logs folder itself. One per line; saved on leaving the field.
 */
function ExtraDirs({ source, json, onSave }: { source: string; json: string | undefined; onSave: (json: string) => void }) {
  let all: Record<string, string[]> = {};
  try {
    all = json ? JSON.parse(json) : {};
  } catch {
    all = {};
  }
  const mine = (all[source] ?? []).join("\n");
  const [text, setText] = useState(mine);
  const [open, setOpen] = useState(mine.length > 0);
  if (!open) {
    return (
      <button className="mt-1 text-xs text-zinc-500 hover:text-zinc-300" onClick={() => setOpen(true)}>
        + Also read logs from another folder
      </button>
    );
  }
  return (
    <label className="mt-2 block">
      <span className="text-xs text-zinc-500">Also read logs from (one folder per line). New folders are watched live after a restart.</span>
      <textarea
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          if (text === mine) return;
          const dirs = text.split("\n").map((d) => d.trim()).filter(Boolean);
          onSave(JSON.stringify({ ...all, [source]: dirs }));
        }}
        placeholder={source === "claude_code" ? "~/.claude-work" : source === "copilot_cli" ? "~/.copilot-work" : "/path/to/logs"}
        className={cx(inputClass, "mt-1 font-mono text-xs")}
      />
    </label>
  );
}
