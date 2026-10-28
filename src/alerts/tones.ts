// Built-in alert tones, synthesised with WebAudio so no audio files ship
// with the app.

export const TONES = {
  chime: "Chime",
  ping: "Ping",
  knock: "Knock",
  rise: "Rise",
  fall: "Fall",
  soft: "Soft",
} as const;

export type Tone = keyof typeof TONES;

/** [frequency Hz, start s, length s] notes for each tone. */
const NOTES: Record<Tone, { wave: OscillatorType; notes: [number, number, number][] }> = {
  chime: { wave: "sine", notes: [[880, 0, 0.5], [1320, 0.12, 0.6]] },
  ping: { wave: "sine", notes: [[1568, 0, 0.25]] },
  knock: { wave: "triangle", notes: [[220, 0, 0.09], [220, 0.16, 0.09]] },
  rise: { wave: "sine", notes: [[523, 0, 0.18], [659, 0.12, 0.18], [784, 0.24, 0.35]] },
  fall: { wave: "triangle", notes: [[659, 0, 0.2], [440, 0.16, 0.4]] },
  soft: { wave: "sine", notes: [[392, 0, 0.7]] },
};

let ctx: AudioContext | null = null;
export function audio(): AudioContext {
  ctx ??= new AudioContext();
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

export function playTone(tone: Tone, volume: number) {
  const ac = audio();
  const { wave, notes } = NOTES[tone];
  const t0 = ac.currentTime + 0.02;
  for (const [hz, at, len] of notes) {
    const osc = ac.createOscillator();
    const gain = ac.createGain();
    osc.type = wave;
    osc.frequency.value = hz;
    // A quick attack and an exponential release, so tones never click.
    gain.gain.setValueAtTime(0.0001, t0 + at);
    gain.gain.exponentialRampToValueAtTime(Math.max(0.0002, 0.35 * volume), t0 + at + 0.015);
    gain.gain.exponentialRampToValueAtTime(0.0001, t0 + at + len);
    osc.connect(gain).connect(ac.destination);
    osc.start(t0 + at);
    osc.stop(t0 + at + len + 0.05);
  }
}
