type Wide = { kind: "wide"; value: number };
type Narrow = { kind: "narrow"; value: 1 };
function one<T extends { kind: "a"; x: number }>(value: T): number { return value.x; }
function choose<T extends Wide | Narrow>(value: T): T { return value; }
function main(): void {
  assert(one({ ["kind"]: "a", x: 1 }) === 1, "computed literal key");
  const kind = "narrow";
  const selected = choose({ kind, value: 1 });
  const exact: 1 = selected.value;
  const key = "kind";
  const computed = choose({ [key]: kind, value: 1, extra: "kept" });
  assert(exact === 1 && computed.value === 1 && computed.extra === "kept", "constant keys and discriminants preserve context");
}
