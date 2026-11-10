type A = { kind: "a"; x: number };
type B = { kind: "b"; y: string };

function main(): void {
  const either: Partial<A | B> = { kind: "b" };
  assert(either.kind === "b", "partial distributes over a union");

  const holes: Partial<number[]> = [1, undefined];
  assert(holes.length === 2 && holes[1] === undefined, "partial array");
  const dense: Required<(number | undefined)[]> = [1, 2];
  const first: number = dense[0];
  assert(first === 1, "required array");
  const view: Readonly<number[]> = [1, 2, 3];
  assert(view.length === 3, "readonly array");

  const pair: Partial<[number, string]> = [1];
  assert(pair.length === 1 && pair[1] === undefined, "partial tuple");
  const full: Required<[number, string?]> = [1, "s"];
  const second: string = full[1];
  assert(second === "s", "required tuple");

  const text: Partial<string> = "s";
  const flag: Readonly<boolean> = true;
  assert(text === "s" && flag, "primitives pass through");
  const empty: Partial<unknown> = 1;
  assert(empty === 1, "unknown maps to {}");
  const nothing: Partial<never>[] = [];
  assert(nothing.length === 0, "never stays never");
}
