// A template interpolation converts any value as JavaScript's ToString does:
// `null` and `undefined` spell themselves, whatever the static type.
function show<T>(value: T): string { return `<${value}>`; }
function main(): void {
  const n: string | null = null;
  const o: { a?: number } = {};
  const u: unknown = undefined;
  const m = new Map<string, boolean>();
  assert(`n=${n}` === "n=null", "a null union");
  assert(`a=${o.a}` === "a=undefined", "an absent optional field");
  assert(`${u}` === "undefined", "an unknown holding undefined");
  assert(`${m.get("x")}` === "undefined", "a collection miss");
  assert(show(null) + show(undefined) + show(3) === "<null><undefined><3>", "a type parameter");
  o.a = 2;
  assert(`a=${o.a}` === "a=2", "a present value still prints itself");
}
