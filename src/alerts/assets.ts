import { api } from "../api";
import { audio } from "./tones";

// Imported files are read once and kept for the session.
const images = new Map<string, Promise<string>>();
const sounds = new Map<string, Promise<AudioBuffer>>();

function toDataUrl(bytes: ArrayBuffer, mime: string): string {
  let bin = "";
  const view = new Uint8Array(bytes);
  for (let i = 0; i < view.length; i += 0x8000) bin += String.fromCharCode(...view.subarray(i, i + 0x8000));
  return `data:${mime};base64,${btoa(bin)}`;
}

const MIME: Record<string, string> = { png: "image/png", jpg: "image/jpeg", gif: "image/gif", webp: "image/webp" };

export function imageUrl(id: string): Promise<string> {
  let p = images.get(id);
  if (!p) {
    p = api.readAlertAsset(id).then((b) => toDataUrl(b, MIME[id.split(".").pop() ?? ""] ?? "image/png"));
    p.catch(() => images.delete(id));
    images.set(id, p);
  }
  return p;
}

export function soundBuffer(id: string): Promise<AudioBuffer> {
  let p = sounds.get(id);
  if (!p) {
    p = api.readAlertAsset(id).then((b) => audio().decodeAudioData(b.slice(0)));
    p.catch(() => sounds.delete(id));
    sounds.set(id, p);
  }
  return p;
}

export async function playFile(id: string, volume: number) {
  const ac = audio();
  const src = ac.createBufferSource();
  src.buffer = await soundBuffer(id);
  const gain = ac.createGain();
  gain.gain.value = volume;
  src.connect(gain).connect(ac.destination);
  src.start();
}
