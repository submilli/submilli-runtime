// The agent's replies are Markdown, as models write it. They are shown formatted, as
// text only: the result is a tree of plain parts the page draws itself, never HTML, so
// nothing a model writes can become markup, a link, or a script. A link shows as its
// words with the address after them, to read, not to follow.

export type Span =
  | { kind: "text"; text: string }
  | { kind: "strong"; children: Span[] }
  | { kind: "em"; children: Span[] }
  | { kind: "code"; text: string };

export type Block =
  | { kind: "paragraph"; spans: Span[] }
  | { kind: "heading"; spans: Span[] }
  | { kind: "list"; ordered: boolean; start: number; items: Span[][] }
  /** A fenced code block, or a table, kept as typed so its columns line up. */
  | { kind: "pre"; text: string };

const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*(\d{1,9})[.)]\s+(.*)$/;
const HEADING = /^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/;
const FENCE = /^\s{0,3}(```|~~~)/;
const TABLE = /^\s*\|.*\|\s*$/;

export function parseMarkdown(text: string): Block[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let para: string[] = [];
  const endPara = () => {
    if (para.length) blocks.push({ kind: "paragraph", spans: parseInline(para.join("\n")) });
    para = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const fence = FENCE.exec(line);
    if (fence) {
      endPara();
      const body: string[] = [];
      for (i++; i < lines.length && !lines[i].trimStart().startsWith(fence[1]); i++) body.push(lines[i]);
      blocks.push({ kind: "pre", text: body.join("\n") });
      continue;
    }
    if (TABLE.test(line)) {
      endPara();
      const rows = [line];
      while (i + 1 < lines.length && TABLE.test(lines[i + 1])) rows.push(lines[++i]);
      blocks.push({ kind: "pre", text: rows.join("\n") });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      endPara();
      blocks.push({ kind: "heading", spans: parseInline(heading[1]) });
      continue;
    }
    const bullet = BULLET.exec(line);
    const numbered = bullet ? null : NUMBERED.exec(line);
    if (bullet || numbered) {
      endPara();
      const ordered = !!numbered;
      const items: string[] = [bullet ? bullet[1] : numbered![2]];
      // The next items of the same kind, and lines that carry an item on.
      while (i + 1 < lines.length) {
        const next = lines[i + 1];
        const more = ordered ? NUMBERED.exec(next) : BULLET.exec(next);
        if (more) {
          items.push(ordered ? more[2] : more[1]);
          i++;
        } else if (next.trim() && /^\s{2,}\S/.test(next) && !BULLET.test(next) && !NUMBERED.test(next)) {
          items[items.length - 1] += " " + next.trim();
          i++;
        } else break;
      }
      blocks.push({ kind: "list", ordered, start: numbered ? Number(numbered[1]) : 1, items: items.map(parseInline) });
      continue;
    }
    if (!line.trim()) {
      endPara();
      continue;
    }
    para.push(line);
  }
  endPara();
  return blocks;
}

/** Bold, italic, code and links in one line or paragraph; anything else is text as written. */
export function parseInline(text: string): Span[] {
  const spans: Span[] = [];
  let plain = "";
  const flush = () => {
    if (plain) spans.push({ kind: "text", text: plain });
    plain = "";
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    // `code` keeps everything inside it as written.
    const code = /^(`+)([\s\S]*?[^`])\1(?!`)/.exec(rest);
    if (code) {
      flush();
      spans.push({ kind: "code", text: code[2].trim() || code[2] });
      i += code[0].length;
      continue;
    }
    // A link, read not followed: its words, then the address.
    const link = /^\[([^\]\n]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/.exec(rest);
    if (link) {
      flush();
      spans.push(...parseInline(link[1]));
      plain = link[2] === link[1] ? "" : " (" + link[2] + ")";
      i += link[0].length;
      continue;
    }
    const strong = /^(\*\*|__)(?=\S)([\s\S]*?\S)\1/.exec(rest);
    if (strong) {
      flush();
      spans.push({ kind: "strong", children: parseInline(strong[2]) });
      i += strong[0].length;
      continue;
    }
    // A single * or _ around words, not inside one (snake_case, 2*3*4 stay as they are).
    const before = i > 0 ? text[i - 1] : " ";
    const em = /^([*_])(?=\S)([^*_\n]*?\S)\1(?![A-Za-z0-9])/.exec(rest);
    if (em && !/[A-Za-z0-9]/.test(before)) {
      flush();
      spans.push({ kind: "em", children: parseInline(em[2]) });
      i += em[0].length;
      continue;
    }
    plain += text[i];
    i++;
  }
  flush();
  return spans;
}
