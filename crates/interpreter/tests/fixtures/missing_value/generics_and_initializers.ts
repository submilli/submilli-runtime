// SUB-2148: a value typed with a type parameter may be a present `undefined`,
// which is not a missing one: a local holding such a read keeps a present
// flag. And the initializers of module-level bindings, class fields, static
// fields and parameter defaults take a missing read where their type names
// `undefined`.
const XS: number[] = [];
const G: number | undefined = XS[0];

class Holder {
  static s: number | undefined = XS[4];
  f: number | undefined = XS[1];
  g?: number = XS[2];
}

interface Item {
  name: string;
}

function nth<T>(xs: T[], i: number): T | undefined {
  const v = xs[i];
  if (v === undefined) return undefined;
  return v;
}

function getOr<V>(m: Record<string, V>, k: string, d: V): V {
  const hit = m[k];
  if (hit !== undefined) return hit;
  return d;
}

function unannotatedDefault(x = XS[5]): number {
  return x + 1;
}

function patternParam([o]: Item[]): string {
  for (let i = 0; i < 1; i++) {
    o = o;
  }
  return o.name;
}

function withDefault(x: number | undefined = XS[3]): string {
  return String(x);
}

function swap<T>(a: T[], i: number, j: number): void {
  const t = a[i];
  a[i] = a[j];
  a[j] = t;
}

function firstOf<T>(a: T[]): T {
  const x: T | undefined = a[0];
  return x;
}

function read<T>(r: Record<string, T>, k: string): T {
  return r[k];
}

function main(): void {
  assert(G === undefined, "a module-level initializer");
  const holder = new Holder();
  assert(holder.f === undefined && holder.g === undefined, "class field initializers");
  assert(withDefault() === "undefined", "a parameter default");
  assert(Holder.s === undefined, "a static field initializer");
  assert(nth([1], 5) === undefined, "a generic local checked later");
  assert(getOr<number>({}, "k", 5) === 5, "a generic record read checked later");
  let unannotated = "";
  try {
    unannotatedDefault();
  } catch (e: MissingValueError) {
    unannotated = e.name;
  }
  assert(unannotated === "MissingValueError", "an unannotated default reads the value");
  let pattern = "";
  try {
    patternParam([]);
  } catch (e: MissingValueError) {
    pattern = e.name;
  }
  assert(pattern === "MissingValueError", "a boxed pattern parameter checks the value");

  const values: (number | undefined)[] = [1, undefined];
  swap(values, 0, 1);
  assert(values[0] === undefined && values[1] === 1, "a present undefined element is swapped");
  assert(firstOf([undefined]) === undefined, "a present undefined element is returned");
  const record: Record<string, string | undefined> = { k: undefined };
  assert(read(record, "k") === undefined, "a present undefined value");
  let missing = "";
  try {
    read(record, "zz");
  } catch (e: MissingValueError) {
    missing = e.message;
  }
  assert(missing === "key \"zz\" is missing from `r`; use `r[key] ?? …` or check `key in r` first", missing);

  const xs: { name: string }[] = [];
  assert((xs[0] && xs[0].name || "or") === "or", "&& on the left of ||");
}
