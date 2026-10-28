// Themes and skins.
//
// The UI is written with Tailwind's `zinc` (surfaces and text) and `emerald`
// (accent) classes. Tailwind v4 compiles those to CSS variables, so a theme
// only has to redefine the variables: `--color-zinc-*` become the theme's
// neutral scale and `--color-emerald-*` the chosen accent. Light themes
// invert the scales so "zinc-950 background, zinc-100 text" stays readable.

const SHADES = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;
type Scale = readonly [string, string, string, string, string, string, string, string, string, string, string];

interface ThemeDef {
  id: string;
  label: string;
  /** Neutral scale, lightest (50) to darkest (950), as written for a dark UI. */
  neutral: Scale;
  light: boolean;
}

const zinc: Scale = ["#fafafa", "#f4f4f5", "#e4e4e7", "#d4d4d8", "#a1a1aa", "#71717a", "#52525b", "#3f3f46", "#27272a", "#18181b", "#09090b"];
const solarized: Scale = ["#fdf6e3", "#eee8d5", "#d9d4c0", "#b8bdb5", "#93a1a1", "#839496", "#657b83", "#2f5560", "#0a3d4a", "#04313d", "#002b36"];

export const THEMES: ThemeDef[] = [
  { id: "midnight", label: "Midnight", light: false, neutral: zinc },
  { id: "graphite", label: "Graphite", light: false, neutral: ["#fafafa", "#f5f5f5", "#e5e5e5", "#d4d4d4", "#a3a3a3", "#737373", "#525252", "#404040", "#262626", "#171717", "#0a0a0a"] },
  { id: "slate", label: "Slate", light: false, neutral: ["#f8fafc", "#f1f5f9", "#e2e8f0", "#cbd5e1", "#94a3b8", "#64748b", "#475569", "#334155", "#1e293b", "#0f172a", "#020617"] },
  { id: "nord", label: "Nord", light: false, neutral: ["#eceff4", "#e5e9f0", "#d8dee9", "#c0c8d6", "#a3adbf", "#7b88a1", "#616e88", "#4c566a", "#434c5e", "#3b4252", "#2e3440"] },
  { id: "dracula", label: "Dracula", light: false, neutral: ["#f8f8f2", "#e9e9e4", "#d6d6d1", "#bfbfc8", "#a0a3b8", "#7c80a0", "#6272a4", "#4d5170", "#44475a", "#343746", "#282a36"] },
  { id: "solarized-dark", label: "Solarized Dark", light: false, neutral: solarized },
  { id: "light", label: "Light", light: true, neutral: zinc },
  { id: "solarized-light", label: "Solarized Light", light: true, neutral: solarized },
];

// `onAccent` is the text colour on a solid accent button: dark on light hues
// (emerald, cyan, amber), white on deep ones: whichever reads better.
export const ACCENTS: { id: string; label: string; scale: Scale; onAccent: string }[] = [
  // Tilly's brand accent, between the logo's pink and purple.
  { id: "tilly", label: "Tilly", scale: ["#fdf4ff", "#fae8ff", "#f5d0fe", "#f0abfc", "#e879f9", "#d946ef", "#c026d3", "#a21caf", "#86198f", "#701a75", "#4a044e"], onAccent: "#ffffff" },
  { id: "emerald", label: "Emerald", scale: ["#ecfdf5", "#d1fae5", "#a7f3d0", "#6ee7b7", "#34d399", "#10b981", "#059669", "#047857", "#065f46", "#064e3b", "#022c22"], onAccent: "#04130d" },
  { id: "blue", label: "Blue", scale: ["#eff6ff", "#dbeafe", "#bfdbfe", "#93c5fd", "#60a5fa", "#3b82f6", "#2563eb", "#1d4ed8", "#1e40af", "#1e3a8a", "#172554"], onAccent: "#ffffff" },
  { id: "violet", label: "Violet", scale: ["#f5f3ff", "#ede9fe", "#ddd6fe", "#c4b5fd", "#a78bfa", "#8b5cf6", "#7c3aed", "#6d28d9", "#5b21b6", "#4c1d95", "#2e1065"], onAccent: "#ffffff" },
  { id: "cyan", label: "Cyan", scale: ["#ecfeff", "#cffafe", "#a5f3fc", "#67e8f9", "#22d3ee", "#06b6d4", "#0891b2", "#0e7490", "#155e75", "#164e63", "#083344"], onAccent: "#04161b" },
  { id: "amber", label: "Amber", scale: ["#fffbeb", "#fef3c7", "#fde68a", "#fcd34d", "#fbbf24", "#f59e0b", "#d97706", "#b45309", "#92400e", "#78350f", "#451a03"], onAccent: "#1c1003" },
  { id: "rose", label: "Rose", scale: ["#fff1f2", "#ffe4e6", "#fecdd3", "#fda4af", "#fb7185", "#f43f5e", "#e11d48", "#be123c", "#9f1239", "#881337", "#4c0519"], onAccent: "#ffffff" },
];

// Status colours used for warnings and outcomes. Light themes invert them too
// so their `-300` text shades stay legible on a light background.
const STATUS: Record<string, Scale> = {
  amber: ACCENTS.find((a) => a.id === "amber")!.scale,
  rose: ACCENTS.find((a) => a.id === "rose")!.scale,
  sky: ["#f0f9ff", "#e0f2fe", "#bae6fd", "#7dd3fc", "#38bdf8", "#0ea5e9", "#0284c7", "#0369a1", "#075985", "#0c4a6e", "#082f49"],
  // Agent statuses, kept apart from the accent so a green or amber accent
  // never reads as "working" or "your turn".
  green: ["#f0fdf4", "#dcfce7", "#bbf7d0", "#86efac", "#4ade80", "#22c55e", "#16a34a", "#15803d", "#166534", "#14532d", "#052e16"],
  orange: ["#fff7ed", "#ffedd5", "#fed7aa", "#fdba74", "#fb923c", "#f97316", "#ea580c", "#c2410c", "#9a3412", "#7c2d12", "#431407"],
  violet: ["#f5f3ff", "#ede9fe", "#ddd6fe", "#c4b5fd", "#a78bfa", "#8b5cf6", "#7c3aed", "#6d28d9", "#5b21b6", "#4c1d95", "#2e1065"],
  // Scheduled: a calm blue-violet, clear of the pink-purple brand accent.
  indigo: ["#eef2ff", "#e0e7ff", "#c7d2fe", "#a5b4fc", "#818cf8", "#6366f1", "#4f46e5", "#4338ca", "#3730a3", "#312e81", "#1e1b4b"],
};

export const DENSITIES = [
  { id: "compact", label: "Compact", rootPx: 14.5 },
  { id: "comfortable", label: "Comfortable", rootPx: 16 },
  { id: "spacious", label: "Spacious", rootPx: 17.5 },
];

export const CORNERS = [
  { id: "rounded", label: "Rounded", scale: 1 },
  { id: "subtle", label: "Subtle", scale: 0.5 },
  { id: "sharp", label: "Sharp", scale: 0 },
];

export interface Appearance {
  theme: string;
  accent: string;
  density: string;
  corners: string;
}

const DEFAULT_APPEARANCE: Appearance = { theme: "midnight", accent: "tilly", density: "comfortable", corners: "rounded" };

/** Ready-made combinations offered as one-click skins. */
export const PRESETS: { label: string; appearance: Appearance }[] = [
  { label: "Default", appearance: DEFAULT_APPEARANCE },
  { label: "Daylight", appearance: { theme: "light", accent: "blue", density: "comfortable", corners: "rounded" } },
  { label: "Arctic", appearance: { theme: "nord", accent: "cyan", density: "comfortable", corners: "subtle" } },
  { label: "Vampire", appearance: { theme: "dracula", accent: "violet", density: "comfortable", corners: "rounded" } },
  { label: "Terminal", appearance: { theme: "graphite", accent: "emerald", density: "compact", corners: "sharp" } },
  { label: "Paper", appearance: { theme: "solarized-light", accent: "amber", density: "spacious", corners: "subtle" } },
];

const STORAGE_KEY = "appearance";

export function loadAppearance(): Appearance {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...DEFAULT_APPEARANCE, ...(JSON.parse(raw) as Partial<Appearance>) };
  } catch {
    // Unavailable storage just means defaults.
  }
  return DEFAULT_APPEARANCE;
}

export function saveAppearance(a: Appearance) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(a));
  } catch {
    // Not fatal; the choice applies for this run.
  }
}

function setScale(style: CSSStyleDeclaration, name: string, scale: Scale, invert: boolean) {
  SHADES.forEach((shade, i) => style.setProperty(`--color-${name}-${shade}`, scale[invert ? SHADES.length - 1 - i : i]));
}

// Tailwind's default radii in rem.
const RADII: Record<string, number> = { sm: 0.25, md: 0.375, lg: 0.5, xl: 0.75 };

export function applyAppearance(a: Appearance) {
  const theme = THEMES.find((t) => t.id === a.theme) ?? THEMES[0];
  const accent = ACCENTS.find((x) => x.id === a.accent) ?? ACCENTS[0];
  const density = DENSITIES.find((d) => d.id === a.density) ?? DENSITIES[1];
  const corners = CORNERS.find((c) => c.id === a.corners) ?? CORNERS[0];
  const root = document.documentElement;
  const style = root.style;

  setScale(style, "zinc", theme.neutral, theme.light);
  setScale(style, "emerald", accent.scale, theme.light);
  style.setProperty("--color-on-accent", accent.onAccent);
  for (const [name, scale] of Object.entries(STATUS)) setScale(style, name, scale, theme.light);
  style.setProperty("color-scheme", theme.light ? "light" : "dark");
  style.fontSize = `${density.rootPx}px`;
  for (const [k, rem] of Object.entries(RADII)) style.setProperty(`--radius-${k}`, `${rem * corners.scale}rem`);
  root.dataset.theme = theme.id;
}

/** Chart colours that follow the theme. */
export const chart = {
  grid: "var(--color-zinc-800)",
  axis: "var(--color-zinc-500)",
  muted: "var(--color-zinc-700)",
  accent: "var(--color-emerald-500)",
  accentSoft: "var(--color-emerald-700)",
  /** Axis styling shared by every chart: small labels, no lines. */
  axisProps: { tick: { fill: "var(--color-zinc-500)", fontSize: 11 }, tickLine: false, axisLine: false },
  tooltip: {
    contentStyle: {
      background: "var(--color-zinc-900)",
      border: "1px solid var(--color-zinc-700)",
      borderRadius: 8,
      fontSize: 12,
    },
    labelStyle: { color: "var(--color-zinc-400)" },
    itemStyle: { color: "var(--color-zinc-200)" },
  },
};

/** Sequential colour for heatmap cells, 0..1. */
export function heat(x: number): string {
  if (x <= 0) return "var(--color-zinc-800)";
  const pct = Math.round(25 + Math.min(1, x) * 75);
  return `color-mix(in oklab, var(--color-emerald-500) ${pct}%, var(--color-zinc-800))`;
}
