// A prelude method whose signature mentions the interface's type parameter is
// dispatched through a wrapper with erased `(ref null $Object)` slots. The
// chain inferer substitutes the signature to concrete types before codegen
// sees it, so the boxing and the return cast have to come from the wrapper's
// recorded ABI.

function pushOne(a: number[] | null): number | undefined {
  return a?.push(1);
}

function firstOf(a: number[] | null): number | undefined {
  return a?.at(0);
}

function firstName(a: string[] | null): string | undefined {
  return a?.at(0);
}

function lookup(m: Map<string, number> | null): number | undefined {
  return m?.get("k");
}

function taken(s: Set<number> | null): boolean | undefined {
  return s?.has(2);
}

function main(): void {
  const arr: number[] = [7];
  assert(pushOne(arr) === 2, "primitive arg boxed into the erased slot");
  assert(arr.length === 2, "push reached the array");
  assert(pushOne(null) === undefined, "null receiver short-circuits");

  assert(firstOf(arr) === 7, "erased return cast back to number");
  assert(firstOf([]) === undefined, "the method's own undefined survives the cast");
  assert(firstOf(null) === undefined, "null receiver short-circuits");

  assert(firstName(["hi"]) === "hi", "erased return cast back to string");
  assert(firstName([]) === undefined, "empty array yields undefined");

  const m = new Map<string, number>();
  m.set("k", 5);
  assert(lookup(m) === 5, "erased key arg and erased return");
  assert(lookup(new Map<string, number>()) === undefined, "missing key yields undefined");
  assert(lookup(null) === undefined, "null receiver short-circuits");

  const s = new Set<number>();
  s.add(2);
  assert(taken(s) === true, "boxed arg with a boolean return");
  assert(taken(null) === undefined, "null receiver short-circuits");
}
