/** The Governess umbrella. Drawn in theme colours, so it follows the accent. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="190 150 680 640" aria-hidden>
      <path
        d="M222 560 C222 380 352 262 512 262 C672 262 802 380 802 560 Q 729.5 512 657 560 Q 584.5 512 512 560 Q 439.5 512 367 560 Q 294.5 512 222 560 Z"
        fill="var(--color-emerald-500)"
      />
      <circle cx="512" cy="250" r="22" fill="var(--color-zinc-100)" />
      <path d="M512 552 L512 742 A62 62 0 0 1 388 742" fill="none" stroke="var(--color-zinc-100)" strokeWidth="48" strokeLinecap="round" />
      <path d="M770 170 Q778 222 830 230 Q778 238 770 290 Q762 238 710 230 Q762 222 770 170 Z" fill="var(--color-amber-300)" />
    </svg>
  );
}
