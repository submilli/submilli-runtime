function identity<T>(x: T): T { return x; }
function pair(a: number, b?: string): [number, string] { return [a, b ?? ""]; }
function count(...xs: number[]): number { return xs.length; }
function songs(): { id: number; title: string }[] { return [{ id: 1, title: "one" }]; }
const measure = (s: string): { len: number } => ({ len: s.length });
type Song = ReturnType<typeof songs>;

function main(): void {
  const library: ReturnType<typeof songs> = [];
  const found = [] as ReturnType<typeof songs>;
  library.push(...songs());
  found.push({ id: 2, title: "two" });
  const alias: Song = library;
  assert(library.length === 1 && found[0].title === "two" && alias[0].id === 1, "return type of a function");

  const args: Parameters<typeof pair> = [1];
  const full: Parameters<typeof pair> = [1, "b"];
  const rest: Parameters<typeof count> = [1, 2, 3];
  assert(args.length === 1 && full[1] === "b" && count(...rest) === 3, "parameters");

  const size: ReturnType<typeof measure> = measure("abc");
  const nothing: ReturnType<() => void> = undefined;
  const either: ReturnType<(() => number) | (() => string)> = "s";
  const generic: ReturnType<typeof identity> = "anything";
  assert(size.len === 3 && nothing === undefined && either === "s" && generic === "anything", "function types");
}
