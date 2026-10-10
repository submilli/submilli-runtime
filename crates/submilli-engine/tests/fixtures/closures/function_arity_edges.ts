// Functions that declare fewer parameters than the function type they stand
// for, where the runtime passes the arguments: host callbacks, comparators,
// equality, and writes that narrow to the declared function type.

type Un = (a: number) => number;
type Bin = (a: number, b: number) => number;

function addDefault(x: number, y: number = 100): number {
  return x + y;
}

class Bag<T> {
  private items: T[] = [];
  private byKey: Map<string, T> = new Map<string, T>();
  add(x: T): void {
    this.items.push(x);
    this.byKey.set(String(this.items.length), x);
  }
  // Callbacks on private fields of a generic class take the dynamic call path.
  show(f: (x: T) => string): string {
    const indexed = this.items.map((x, i) => f(x) + String(i)).join(",");
    let keyed = "";
    this.byKey.forEach((v) => {
      keyed += f(v);
    });
    const folded = this.items.reduce((acc, x) => acc + f(x), "");
    const found = this.items.findIndex((x) => f(x) === "2");
    return [indexed, keyed, folded, String(found)].join("|");
  }
}

function comparators(): void {
  const xs = [3, 1, 2];
  assert(xs.slice().sort(() => 0).join(",") === "3,1,2", "sort, no params");
  // A stable sort keeps the order when every pair compares equal.
  assert(xs.toSorted((a) => a - a).join(",") === "3,1,2", "toSorted, one param");
  assert(new Uint8Array([2, 1]).sort(() => 0).join(",") === "2,1", "Uint8Array sort, no params");
}

function equality(): void {
  const un: Un = (a) => a + 1;
  const bin: Bin = un;
  assert(bin === un, "a function equals itself through a wider type");
  const other: Bin = (a, b) => a + b;
  assert(un !== other, "different functions of different arities");
  const nullable: Bin | null = un;
  const same: Un | null = un;
  assert(nullable === same, "nullable function types");
}

function callbacks(): void {
  const bag = new Bag<number>();
  bag.add(1);
  bag.add(2);
  assert(bag.show((x) => String(x)) === "10,21|12|12|1", "dynamic member path");
  // `map` passes the index, which fills the default parameter.
  assert([1, 2, 3].map(addDefault).join(",") === "1,3,5", "index fills a default");
  assert([1, 2, 3].reduce(() => 7, 0) === 7, "reduce, no params");
  assert([1, 2].reduce((acc, v, i, all) => acc + v * i + all.length, 0) === 6, "reduce, all params");
  assert(Array.from(new Set(["a", "b"]), (v, i) => v + String(i)).join("") === "a0b1", "Array.from a Set");
  assert(Array.from("xy", (c, i) => c + String(i)).join("") === "x0y1", "Array.from a string");
  const bytes = new Uint8Array([1, 2, 3]);
  assert(bytes.reduceRight((acc, b, i) => acc + b * i, 0) === 8, "Uint8Array reduceRight index");
  assert(bytes.every((b, i) => b === i + 1), "Uint8Array every index");
  assert(bytes.findIndex((b, i, all) => i === all.length - 1) === 2, "Uint8Array findIndex array");
  const frozen: readonly number[] = [4, 5];
  assert(frozen.map((v, i) => v + i).join(",") === "4,6", "readonly array");
  const pair: [number, number] = [6, 7];
  assert(pair.map((v, i) => v * i).join(",") === "0,7", "tuple");
  // forEach visits only the elements present when it starts.
  const grow = [1, 2];
  grow.forEach((v) => {
    grow.push(v);
  });
  assert(grow.join(",") === "1,2,1,2", "forEach skips pushed elements");
}

function containers(): void {
  let fns: Bin[] | null = null;
  fns = [(a: number) => a + 5, (a, b) => a * b];
  assert(fns[0](1, 2) === 6 && fns[1](3, 4) === 12, "into an array");
  let tuple: [Bin, string] | null = null;
  tuple = [(a: number) => a - 1, "t"];
  assert(tuple[0](10, 0) === 9, "into a tuple");
  let holder: { f: Bin; n: number } | null = null;
  holder = { f: (a: number) => a + 5, n: 1 };
  assert(holder.f(1, 2) === 6, "into an object field");
}

function callWith<T>(f: (t: T) => number, t: T): number {
  return f(t);
}

function measure(s: string): (n: number) => number {
  return (n) => n + s.length;
}

function erasedAndOrdered(): void {
  const un: Un = (a) => a * 2;
  const bin: Bin = un;
  const erased: unknown = bin;
  // The adapter `bin` holds wraps `un`, which really takes one argument.
  assert((erased as Un)(4) === 8, "cast an adapter back to the original's type");
  assert([bin].includes(un) && new Set<Bin>([un, bin]).size === 1, "identity in collections");
  // Through adapters, a function still gets every argument it is passed:
  // `addDefault`'s second parameter takes the index, not its default.
  const narrowed = (addDefault as unknown) as Un;
  const widened: Bin = narrowed;
  assert(widened(1, 2) === 3, "an adapter of an adapter passes every argument");
  assert([1, 2].map(narrowed).join(",") === "1,3", "a host callback gets every argument");
  assert(Array.from([1, 2], narrowed).join(",") === "1,3", "so does Array.from's map function");
  // `measure(s)` is checked before the next argument assigns `s`.
  let s: string | null = "ab";
  assert(callWith(measure(s), (s = null) === null ? 1 : 2) === 3, "arguments in source order");
}

function main(): void {
  comparators();
  equality();
  callbacks();
  containers();
  erasedAndOrdered();
}
