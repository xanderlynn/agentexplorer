import { api } from "../api";

// Imported images are read once and kept for the session.
const images = new Map<string, Promise<string>>();

function toDataUrl(bytes: ArrayBuffer, mime: string): string {
  let bin = "";
  const view = new Uint8Array(bytes);
  for (let i = 0; i < view.length; i += 0x8000) bin += String.fromCharCode(...view.subarray(i, i + 0x8000));
  return `data:${mime};base64,${btoa(bin)}`;
}

const MIME: Record<string, string> = { png: "image/png", jpg: "image/jpeg", gif: "image/gif", webp: "image/webp" };

/** Loads a file once; a failed load is forgotten so it can be retried. */
function cached<T>(map: Map<string, Promise<T>>, id: string, load: (bytes: ArrayBuffer) => T | Promise<T>): Promise<T> {
  let p = map.get(id);
  if (!p) {
    p = api.readAlertAsset(id).then(load);
    p.catch(() => map.delete(id));
    map.set(id, p);
  }
  return p;
}

export const imageUrl = (id: string) => cached(images, id, (b) => toDataUrl(b, MIME[id.split(".").pop() ?? ""] ?? "image/png"));
