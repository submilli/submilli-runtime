function objectKind({ kind }: { kind: "A" | "B" }): "A" | "B" {
  const initial: "A" | "B" = kind;
  kind = "B";
  assert(kind === "B");
  return initial;
}

function tupleValue([value = 1]: [(1 | 3)?] = []): 1 | 3 {
  const initial: 1 | 3 = value;
  value = 3;
  assert(value === 3);
  return initial;
}

function capturedValue(read: () => 1 | 3 = () => value, [value = 1]: [(1 | 3)?] = []): 1 | 3 {
  value = 3;
  return read();
}

function main(): void {
  assert(objectKind({ kind: "A" }) === "A");
  assert(tupleValue() === 1 && tupleValue([3]) === 3);
  assert(capturedValue() === 3, "captured mutable parameter keeps its declared literal union");
  let fresh = 1;
  fresh = 4;
  assert(fresh === 4, "ordinary let inference still widens fresh literals");
}
