class Inner {
  constructor(
    public n: number,
    public label: string,
  ) {}
}

function count_of(i: Inner | null): number | undefined {
  return i?.n;
}

function label_of(i: Inner | null): string | undefined {
  return i?.label;
}

function main(): void {
  const present = new Inner(5, "five");
  assert(count_of(present) === 5, "class field read on non-null");
  assert(count_of(null) === undefined, "short-circuit on null receiver");
  assert(label_of(present) === "five", "string-typed class field");
  assert(label_of(null) === undefined, "short-circuit on string field");

  // method dispatch through `?.` already worked; keep both shapes together
  assert(present.n.toString() === "5");
}
