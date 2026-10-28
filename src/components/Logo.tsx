/** Tilly's mark: a girl with a bob haircut on the brand's pink-to-purple tile. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden>
      <defs>
        <linearGradient id="tilly-logo" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#EC4899" />
          <stop offset="1" stopColor="#8B5CF6" />
        </linearGradient>
        <clipPath id="tilly-logo-tile">
          <rect width="1024" height="1024" rx="230" />
        </clipPath>
      </defs>
      <rect width="1024" height="1024" rx="230" fill="url(#tilly-logo)" />
      <g clipPath="url(#tilly-logo-tile)">
        <path d="M512 200 C700 200 790 330 790 500 L790 690 Q790 720 760 720 L264 720 Q234 720 234 690 L234 500 C234 330 324 200 512 200 Z" fill="#ffffff" />
        <path d="M352 440 C380 420 470 400 512 360 C560 410 640 425 672 440 L672 560 C672 650 600 712 512 712 C424 712 352 650 352 560 Z" fill="#FBCFE8" />
        <path d="M462 700 L562 700 L566 780 C700 800 820 850 850 1024 L174 1024 C204 850 324 800 458 780 Z" fill="#ffffff" />
      </g>
    </svg>
  );
}
