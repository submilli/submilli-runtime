// Array.from over collection and string sources: Map yields [k, v] pairs and
// Set yields values, both in insertion order; strings split at code-point
// boundaries (lone surrogates pass through one unit wide).
function main(): void {
  const m = new Map<string, number>();
  m.set("b", 2);
  m.set("a", 1);
  const pairs = Array.from(m);
  assert(pairs.length === 2, "map yields one pair per entry");
  assert(pairs[0][0] === "b" && pairs[0][1] === 2, "insertion order, [key, value]");
  assert(pairs[1][0] === "a" && pairs[1][1] === 1, "second entry");

  const s = new Set<number>();
  s.add(7);
  s.add(3);
  s.add(7);
  const values = Array.from(s);
  assert(values.length === 2 && values[0] === 7 && values[1] === 3, "set yields deduped values in order");

  const mapped = Array.from("h😀i", (c: string): string => c + "!");
  assert(mapped.length === 3, "mapFn runs on the string path");
  assert(mapped[1] === "😀!", "code point survives the map");

  const live: number[] = [1, 2];
  const grown = Array.from(live, (x: number): number => {
    if (live.length < 4) {
      live.push(9);
    }
    return x;
  });
  assert(grown.length === 4, "mapFn appending to the source is observed (live cursor)");
  assert(grown[2] === 9 && grown[3] === 9, "appended elements flow through mapFn");

  assert(Array.from<number>([]).length === 0, "empty array");
  assert(Array.from("").length === 0, "empty string");
  assert(Array.from(new Map<string, number>()).length === 0, "empty map");
  assert(Array.from(new Set<number>()).length === 0, "empty set");
}
