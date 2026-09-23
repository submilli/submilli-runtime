// `o?.["key"]` reads a field the same way `o["key"]` does — the spelling that
// reaches keys an identifier can't name.
type Headers = { "content-type": string; length: number };

function contentType(h: Headers | null): string | null {
  return h?.["content-type"] ?? null;
}

type Wrapper = { "inner-value": Headers | null; describe(): string };

function main(): void {
  const w: Wrapper | null = {
    "inner-value": { "content-type": "a", length: 1 },
    describe(): string {
      return "wrapper";
    },
  };
  assert(w?.["inner-value"]?.["content-type"] === "a", "chained string keys");
  assert(w?.["describe"]() === "wrapper", "method called through a string key");
  const xs: number[] | null = [1, 2, 3];
  const word: string | null = "four";
  const pair: [number, string] | null = [1, "a"];
  const sizes: Map<string, number> | null = new Map<string, number>();
  assert(xs?.["length"] === 3, "array length");
  assert(word?.["length"] === 4, "string length");
  assert(pair?.["length"] === 2, "tuple length");
  assert(sizes?.["size"] === 0, "Map size");

  const empty: Wrapper | null = null;
  assert((empty?.["inner-value"]?.["length"] ?? -1) === -1, "short-circuits the whole chain");

  const h: Headers = { "content-type": "text/plain", length: 3 };
  assert(contentType(h) === "text/plain", "reads through a present receiver");
  assert(contentType(null) === null, "short-circuits on null");
  const maybe: Headers | null = h;
  assert(maybe?.["length"] === h["length"], "same field as the non-optional spelling");
}
