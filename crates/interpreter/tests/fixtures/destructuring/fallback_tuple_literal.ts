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

function parenthesized(source: [number, string] | undefined): string {
  const [n, s] = source ?? ([]);
  return `${n}:${s}`;
}

// A defaulted slot past a tuple's end reads `undefined`, as in tsc, so its
// default applies.
function padded(): string {
  const [a, b = 5] = ([1]);
  const [c, d = 6] = Math.random() < 2 ? [1] : [2];
  const fixed: [number] = [7];
  const [e, f = 8] = fixed;
  return [a, b, c, d, e, f].join(",");
}

function main(): void {
  assert(pair([1, 2]) === "1/2" && pair(undefined) === "undefined/undefined", "?? []");
  assert(orEmpty(["x"]) === "x" && orEmpty(null) === "none", "|| []");
  assert(parenthesized(undefined) === "undefined:undefined", "?? ([])");
  assert(padded() === "1,5,1,6,7,8", "defaulted slots past a tuple's end");
  assert(chosen(true) === "1:one" && chosen(false) === "undefined:undefined", "a conditional's branches");
}
