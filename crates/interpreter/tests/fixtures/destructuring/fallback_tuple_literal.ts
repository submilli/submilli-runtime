// A pattern takes apart an array literal fallback as a tuple, as in tsc: the
// empty `[]` after `??` or `||`, or in a conditional's branch, holds no element,
// so the pattern's reads may be `undefined`.
function pair(source: [number, number] | undefined): string {
  const [first, second = first] = source ?? [];
  const head: number | undefined = first;
  const tail: number | undefined = second;
  return `${head}/${tail}`;
}

function orEmpty(source: [string] | null): string {
  const [only] = source || [];
  const read: string | undefined = only;
  return read ?? "none";
}

function chosen(flag: boolean): string {
  const [a, b] = flag ? [1, "one"] : [];
  const left: number | undefined = a;
  const right: string | undefined = b;
  return `${left}:${right}`;
}

function main(): void {
  assert(pair([1, 2]) === "1/2" && pair(undefined) === "undefined/undefined", "?? []");
  assert(orEmpty(["x"]) === "x" && orEmpty(null) === "none", "|| []");
  assert(chosen(true) === "1:one" && chosen(false) === "undefined:undefined", "a conditional's branches");
}
