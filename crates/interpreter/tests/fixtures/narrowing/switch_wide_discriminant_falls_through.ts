// A switch without `default` over a discriminant that one member types
// widely, or as `null`, can't list every value, so unmatched values continue
// after it.
type Wide = { kind: "a"; value: number } | { kind: string; label: string };
type Nullable = { kind: "a"; value: number } | { kind: null; label: string };
type Flag = { ok: true; value: number } | { ok: false; error: string } | { ok: number; code: string };

function wide(item: Wide): number {
  switch (item.kind) {
    case "a": return 1;
  }
  return 0;
}

function nullable(item: Nullable): number {
  let result = 0;
  switch (item.kind) {
    case "a": result = 1; break;
    case null: result = 2; break;
  }
  return result;
}

function flag(item: Flag): number {
  switch (item.ok) {
    case true: return item.value;
    case false: return -1;
  }
  return 2;
}

function main(): void {
  console.log(wide({ kind: "a", value: 7 }), wide({ kind: "z", label: "l" }));
  console.log(nullable({ kind: null, label: "l" }), flag({ ok: 3, code: "c" }));
}
