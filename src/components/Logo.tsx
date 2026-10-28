/** Tilly's mark: a "t" with a bow on the brand's pink-to-purple tile. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden>
      <defs>
        <linearGradient id="tilly-logo" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#EC4899" />
          <stop offset="1" stopColor="#8B5CF6" />
        </linearGradient>
      </defs>
      <rect width="1024" height="1024" rx="230" fill="url(#tilly-logo)" />
      <path d="M452 300 H572 V420 H700 V530 H572 V700 Q572 770 650 762 H700 V870 Q452 900 452 712 V530 H330 V420 H452 Z" fill="#ffffff" />
      <path d="M512 296 Q400 150 330 214 Q284 284 512 296 Z M512 296 Q624 150 694 214 Q740 284 512 296 Z" fill="#F9A8D4" />
      <circle cx="512" cy="290" r="40" fill="#ffffff" />
    </svg>
  );
}
