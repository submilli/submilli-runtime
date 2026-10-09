// A closure shape named only by a slot annotation inside a function body —
// no declaration mentions it, and no literal of that shape is ever built.
function main(): void {
  const empty = new Map<string, (a: number, b: number, c: number) => number>();
  assert(empty.get("missing") === undefined, "no entry, no literal of that shape");
  assert(empty.size === 0, "map stays empty");
}
