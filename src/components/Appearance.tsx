import { useState } from "react";
import { Check } from "lucide-react";
import { ACCENTS, CORNERS, DENSITIES, PRESETS, THEMES, applyAppearance, loadAppearance, saveAppearance, type Appearance as AppearanceT } from "../theme";
import { Card, Segmented, cx } from "./ui";

export function AppearanceSettings() {
  const [a, setA] = useState<AppearanceT>(loadAppearance);
  const update = (next: AppearanceT) => {
    setA(next);
    applyAppearance(next);
    saveAppearance(next);
  };
  const same = (x: AppearanceT) => x.theme === a.theme && x.accent === a.accent && x.density === a.density && x.corners === a.corners;

  return (
    <Card title="Appearance" className="xl:col-span-2">
      <div className="space-y-5">
        <div>
          <div className="mb-2 text-xs font-medium text-zinc-400">Skins</div>
          <div className="flex flex-wrap gap-2">
            {PRESETS.map((p) => (
              <button
                key={p.label}
                onClick={() => update(p.appearance)}
                className={cx(
                  "flex items-center gap-2 rounded-lg border px-3 py-1.5 text-sm transition-colors",
                  same(p.appearance) ? "border-emerald-500 text-zinc-50" : "border-zinc-700 text-zinc-300 hover:bg-zinc-800",
                )}
              >
                <ThemeDots themeId={p.appearance.theme} accentId={p.appearance.accent} />
                {p.label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <div className="mb-2 text-xs font-medium text-zinc-400">Theme</div>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            {THEMES.map((t) => {
              const bg = t.light ? t.neutral[0] : t.neutral[10];
              const card = t.light ? t.neutral[1] : t.neutral[9];
              const text = t.light ? t.neutral[9] : t.neutral[1];
              const border = t.light ? t.neutral[3] : t.neutral[7];
              return (
                <button
                  key={t.id}
                  onClick={() => update({ ...a, theme: t.id })}
                  className={cx("relative overflow-hidden rounded-lg border-2 text-left", a.theme === t.id ? "border-emerald-500" : "border-transparent")}
                  style={{ background: bg }}
                >
                  <div className="p-2.5">
                    <div className="mb-1.5 rounded px-2 py-1.5" style={{ background: card, border: `1px solid ${border}` }}>
                      <div className="h-1.5 w-10 rounded" style={{ background: text, opacity: 0.8 }} />
                      <div className="mt-1 h-1.5 w-6 rounded bg-emerald-500" />
                    </div>
                    <div className="text-xs font-medium" style={{ color: text }}>{t.label}</div>
                  </div>
                  {a.theme === t.id && <Check size={14} className="absolute right-2 top-2 text-emerald-500" />}
                </button>
              );
            })}
          </div>
        </div>

        <div className="flex flex-wrap items-start gap-8">
          <div>
            <div className="mb-2 text-xs font-medium text-zinc-400">Accent</div>
            <div className="flex gap-2">
              {ACCENTS.map((x) => (
                <button
                  key={x.id}
                  aria-label={x.label}
                  title={x.label}
                  onClick={() => update({ ...a, accent: x.id })}
                  className={cx("h-7 w-7 rounded-full ring-2 ring-offset-2 ring-offset-zinc-900", a.accent === x.id ? "ring-zinc-200" : "ring-transparent")}
                  style={{ background: x.scale[5] }}
                />
              ))}
            </div>
          </div>
          <div>
            <div className="mb-2 text-xs font-medium text-zinc-400">Density</div>
            <Segmented options={DENSITIES.map((d) => ({ value: d.id, label: d.label }))} value={a.density} onChange={(density) => update({ ...a, density })} />
          </div>
          <div>
            <div className="mb-2 text-xs font-medium text-zinc-400">Corners</div>
            <Segmented options={CORNERS.map((c) => ({ value: c.id, label: c.label }))} value={a.corners} onChange={(corners) => update({ ...a, corners })} />
          </div>
        </div>
      </div>
    </Card>
  );
}

function ThemeDots({ themeId, accentId }: { themeId: string; accentId: string }) {
  const t = THEMES.find((x) => x.id === themeId) ?? THEMES[0];
  const accent = ACCENTS.find((x) => x.id === accentId) ?? ACCENTS[0];
  return (
    <span className="flex -space-x-1">
      <span className="h-3.5 w-3.5 rounded-full border border-zinc-600" style={{ background: t.light ? t.neutral[0] : t.neutral[10] }} />
      <span className="h-3.5 w-3.5 rounded-full border border-zinc-600" style={{ background: accent.scale[5] }} />
    </span>
  );
}
