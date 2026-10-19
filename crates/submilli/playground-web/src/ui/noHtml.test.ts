// Recorded content is only ever inserted as text. This fails if any source under `src/`
// reaches for one of the ways to insert markup instead.
import { expect, test } from "vitest";

const sources = import.meta.glob("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

// Built from parts, so this file doesn't name them itself.
const FORBIDDEN = [
  ["dangerously", "SetInnerHTML"],
  ["inner", "HTML"],
  ["outer", "HTML"],
  ["insertAdjacent", "HTML"],
  ["document", ".write"],
].map((parts) => parts.join(""));

test("no source under src/ inserts markup", () => {
  const files = Object.keys(sources).filter((path) => !path.endsWith("/noHtml.test.ts"));
  expect(files.length).toBeGreaterThan(20);
  const found = files.flatMap((path) => FORBIDDEN.filter((word) => sources[path].includes(word)).map((word) => path + ": " + word));
  expect(found).toEqual([]);
});
