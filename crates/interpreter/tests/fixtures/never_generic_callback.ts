function boom(): never { throw new Error("boom"); }
function pick<T>(arr: T[], f: (a: T, b: T) => number): number { return f(arr[0], arr[1]); }
function choose<T>(f: () => number | T): number { return 1; }
function make<T>(f: () => ((n: number) => T), value: T): T { return f()(3); }
function main(): void {
  assert(choose(() => boom()) === 1, "union callback infers its free type parameter");
  assert(make(() => { return n => n; }, 1) === 3, "composite return hint keeps parameter context");
  assert(Array.from("abc").join("") === "abc", "structural inference still binds generics");
  let caught = 0;
  try { pick<number>([1, 2], (a: number, b: number) => boom()); } catch (e) { caught++; }
  try { [2, 1].sort((a: number, b: number) => boom()); } catch (e) { caught++; }
  try { ["b", "a"].sort((a: string, b: string) => boom()); } catch (e) { caught++; }
  assert(caught === 3, "diverging comparators throw");
}
