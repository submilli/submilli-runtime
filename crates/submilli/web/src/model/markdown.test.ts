import { describe, expect, it } from "vitest";
import { parseInline, parseMarkdown } from "./markdown";

describe("the agent's Markdown", () => {
  it("reads bold, a list with bold in it, and a closing line, as a reply has them", () => {
    const reply = "No — I found only those **2 charges**:\n\n- **ch_a1:** $49.00\n- **ch_a2:** $12.50\n\nTotal: **$61.50**.";
    expect(parseMarkdown(reply)).toEqual([
      { kind: "paragraph", spans: [{ kind: "text", text: "No — I found only those " }, { kind: "strong", children: [{ kind: "text", text: "2 charges" }] }, { kind: "text", text: ":" }] },
      {
        kind: "list",
        ordered: false,
        start: 1,
        items: [
          [{ kind: "strong", children: [{ kind: "text", text: "ch_a1:" }] }, { kind: "text", text: " $49.00" }],
          [{ kind: "strong", children: [{ kind: "text", text: "ch_a2:" }] }, { kind: "text", text: " $12.50" }],
        ],
      },
      { kind: "paragraph", spans: [{ kind: "text", text: "Total: " }, { kind: "strong", children: [{ kind: "text", text: "$61.50" }] }, { kind: "text", text: "." }] },
    ]);
  });

  it("keeps a numbered list's start, a heading, code, and a table as typed", () => {
    const blocks = parseMarkdown("## Steps\n3. read `charges.json`\n4. add them up\n\n```ts\nconst x = 1;\n```\n| id | amount |\n|---|---|\n| ch_a1 | 4900 |");
    expect(blocks.map((b) => b.kind)).toEqual(["heading", "list", "pre", "pre"]);
    expect(blocks[1]).toMatchObject({ ordered: true, start: 3 });
    expect(blocks[2]).toEqual({ kind: "pre", text: "const x = 1;" });
    expect(blocks[3]).toEqual({ kind: "pre", text: "| id | amount |\n|---|---|\n| ch_a1 | 4900 |" });
  });

  it("shows a link as its words and address, never as something to follow", () => {
    expect(parseInline("see [the docs](https://example.com/x) now")).toEqual([
      { kind: "text", text: "see " },
      { kind: "text", text: "the docs" },
      { kind: "text", text: " (https://example.com/x) now" },
    ]);
  });

  it("leaves snake_case, sums and lone stars as they are", () => {
    expect(parseInline("customer_id_value and 2*3*4 and a * b")).toEqual([{ kind: "text", text: "customer_id_value and 2*3*4 and a * b" }]);
    expect(parseInline("an *important* word")).toEqual([{ kind: "text", text: "an " }, { kind: "em", children: [{ kind: "text", text: "important" }] }, { kind: "text", text: " word" }]);
  });
});
