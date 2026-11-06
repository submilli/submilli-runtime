function main(): void {
  const set = new Set<number>([3, 1, 3]);
  const values = [...set];
  assert(values.join(",") === "3,1", "Set insertion order");
  const bytes = new Uint8Array([4, 5]);
  assert([...bytes].join(",") === "4,5", "bytes");
  const map = new Map<string, number>([["a", 1], ["b", 2]]);
  assert([...map.keys()].join(",") === "a,b", "map keys");
  assert([...map.values()].join(",") === "1,2", "map values");
  const entries = [...map];
  assert(entries[0][0] === "a" && entries[1][1] === 2, "map entries");
  assert([...map.entries()][1][0] === "b", "entry iterator");
  assert([..."a😀\uD800"].length === 3, "string code points preserve lone surrogates");
  assert([..."a😀\uD800"][2].charCodeAt(0) === 0xD800, "lone surrogate");
  const empty = new Set<number>();
  assert([0, ...empty, ...set, 9].join(",") === "0,3,1,9", "mixed chunks");
  const iterator = set.values();
  assert([...iterator].join(",") === "3,1", "iterator consumed");
  assert([...iterator].length === 0, "exhausted iterator");
  const contextual: number[] = [...set];
  assert(contextual[0] === 3, "contextual element type");
}
