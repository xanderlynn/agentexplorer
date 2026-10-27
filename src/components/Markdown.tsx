// A small Markdown renderer for the weekly retro. It covers what the retro
// prompt asks for (headings, lists, emphasis, code, quotes, tables) and
// builds React elements directly, so model output is never injected as HTML.
// This replaces react-markdown + remark-gfm and their ~100 dependencies.
import type { ReactNode } from "react";

type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "list"; ordered: boolean; items: { text: string; depth: number }[] }
  | { kind: "quote"; text: string }
  | { kind: "code"; text: string }
  | { kind: "table"; header: string[]; rows: string[][] }
  | { kind: "rule" }
  | { kind: "paragraph"; text: string };

const LIST_ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
const TABLE_SEPARATOR = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;

function splitRow(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());
}

function parseBlocks(md: string): Block[] {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }
    if (line.trim().startsWith("```")) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) body.push(lines[i++]);
      i++;
      blocks.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2].trim() });
      i++;
      continue;
    }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      blocks.push({ kind: "rule" });
      i++;
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && TABLE_SEPARATOR.test(lines[i + 1])) {
      const header = splitRow(line);
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && lines[i].includes("|") && lines[i].trim()) rows.push(splitRow(lines[i++]));
      blocks.push({ kind: "table", header, rows });
      continue;
    }
    if (line.trimStart().startsWith(">")) {
      const body: string[] = [];
      while (i < lines.length && lines[i].trimStart().startsWith(">")) body.push(lines[i++].trimStart().replace(/^>\s?/, ""));
      blocks.push({ kind: "quote", text: body.join(" ") });
      continue;
    }
    const item = LIST_ITEM.exec(line);
    if (item) {
      const ordered = /\d/.test(item[2]);
      const items: { text: string; depth: number }[] = [];
      while (i < lines.length) {
        const m = LIST_ITEM.exec(lines[i]);
        if (m) {
          items.push({ text: m[3], depth: Math.min(3, Math.floor(m[1].length / 2)) });
          i++;
        } else if (lines[i].trim() && /^\s{2,}/.test(lines[i]) && items.length) {
          // A wrapped continuation line of the previous item.
          items[items.length - 1].text += " " + lines[i].trim();
          i++;
        } else break;
      }
      blocks.push({ kind: "list", ordered, items });
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^(#{1,6}\s|```|\s*>)/.test(lines[i]) && !LIST_ITEM.test(lines[i])) para.push(lines[i++].trim());
    blocks.push({ kind: "paragraph", text: para.join(" ") });
  }
  return blocks;
}

// Inline: `code`, **bold**, *italic* / _italic_, [text](url). Links render as
// text plus the address, since the app opens nothing from model output.
const INLINE = /(`[^`]+`)|(\*\*[^*]+\*\*|__[^_]+__)|(\*[^*\s][^*]*\*|_[^_\s][^_]*_)|(\[[^\]]+\]\([^)\s]+\))/g;

export function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const m of text.matchAll(INLINE)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    const [token] = m;
    if (m[1]) out.push(<code key={key++}>{token.slice(1, -1)}</code>);
    else if (m[2]) out.push(<strong key={key++}>{inline(token.slice(2, -2))}</strong>);
    else if (m[3]) out.push(<em key={key++}>{inline(token.slice(1, -1))}</em>);
    else if (m[4]) {
      const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(token)!;
      out.push(
        <span key={key++} title={link[2]}>
          {inline(link[1])} <span className="text-zinc-500">({link[2]})</span>
        </span>,
      );
    }
    last = at + token.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function Markdown({ source }: { source: string }) {
  return (
    <>
      {parseBlocks(source).map((b, i) => {
        switch (b.kind) {
          case "heading": {
            const content = inline(b.text);
            return b.level <= 2 ? <h2 key={i}>{content}</h2> : <h3 key={i} className="mt-4 font-semibold text-zinc-100">{content}</h3>;
          }
          case "list": {
            const items = b.items.map((it, j) => (
              <li key={j} style={{ marginLeft: `${it.depth * 1.1}rem` }}>
                {inline(it.text)}
              </li>
            ));
            return b.ordered ? <ol key={i}>{items}</ol> : <ul key={i}>{items}</ul>;
          }
          case "quote":
            return <blockquote key={i}>{inline(b.text)}</blockquote>;
          case "code":
            return (
              <pre key={i} className="my-2 overflow-x-auto rounded-lg bg-zinc-900 p-3 font-mono text-xs text-zinc-300">
                {b.text}
              </pre>
            );
          case "table":
            return (
              <div key={i} className="overflow-x-auto">
                <table>
                  <thead>
                    <tr>{b.header.map((h, j) => <th key={j}>{inline(h)}</th>)}</tr>
                  </thead>
                  <tbody>
                    {b.rows.map((r, j) => (
                      <tr key={j}>{r.map((c, k) => <td key={k}>{inline(c)}</td>)}</tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case "rule":
            return <hr key={i} className="my-4 border-zinc-800" />;
          default:
            return <p key={i}>{inline(b.text)}</p>;
        }
      })}
    </>
  );
}
