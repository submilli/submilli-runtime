// Tuple element labels document the positions and nothing else: a labeled tuple
// is the same type as its unlabeled spelling, in either direction. Labeled and
// unlabeled elements may be mixed, as TypeScript allows since 5.2.
type Span = [start: number, end: number];
type Entry = readonly [key: string, value: number];
type Mixed = [id: number, string];

function width(range: Span): number {
  return range[1] - range[0];
}

function describe(entry: Entry): string {
  const [key, value] = entry;
  return `${key}=${value}`;
}

function main(): void {
  const plain: [number, number] = [2, 5];
  assert(width(plain) === 3, "an unlabeled tuple satisfies a labeled one");
  const range: Span = [1, 4];
  const back: [number, number] = range;
  assert(back[1] === 4, "a labeled tuple satisfies an unlabeled one");
  assert(describe(["a", 1]) === "a=1", "readonly labeled tuple");
  const mixed: Mixed = [7, "x"];
  assert(mixed[0] === 7 && mixed[1] === "x", "mixed labels");
  const make = (): [lo: number, hi: number] => [0, 1];
  assert(make()[1] === 1, "arrow with a labeled tuple return type");
  const keywordLabels: [new: number, type: string] = [1, "t"];
  assert(keywordLabels[1] === "t", "a label may be a keyword");
}
