interface Profile {
  a: number;
  b: string;
  note?: string;
}

interface Named<T> extends Profile {
  value: T;
}

function copy(p: Profile): void {
  const result = { ...p, c: true };
  assert(result.a === 1 && result.b === "profile" && result.c, "interface fields");
  const optional: { note?: string } = result;
  assert(optional.note === p.note, "optional field");
  const overridden = { ...p, b: 42 };
  assert(overridden.b === 42, "later field overrides the interface field");
  const mapped = [p].map(item => ({ ...item, b: "mapped" }));
  assert(mapped[0].a === 1 && mapped[0].b === "mapped", "callback spread");
  const assigned = Object.assign({}, p, { c: true });
  assert(assigned.a === result.a && assigned.c === result.c, "assign parity");
}

function inherited(p: Named<string>): void {
  const result = { ...p };
  assert(result.a === 1 && result.value === "value", "inherited generic fields");
}

function conditional(p: Profile | undefined): void {
  const result = { ...p };
  assert(result.a === p?.a, "nullable interface spread");
}

function main(): void {
  copy({ a: 1, b: "profile" });
  copy({ a: 1, b: "profile", note: "present" });
  inherited({ a: 1, b: "profile", value: "value" });
  conditional(undefined);
  conditional({ a: 1, b: "profile" });
  const alias: { a: number; b: string } = { a: 1, b: "alias" };
  const result = { ...alias, c: true };
  assert(result.a === 1 && result.b === "alias" && result.c, "structural control");
}
