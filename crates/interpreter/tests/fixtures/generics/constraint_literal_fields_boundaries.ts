type Wide = { kind: "wide"; value: number };
type Narrow = { kind: "narrow"; value: 1 };
function choose<T extends Wide | Narrow>(value: T): T { return value; }
function arrays<T extends Wide[] | Narrow[]>(value: T): T { return value; }
function flags<T extends { flag: boolean; count: number }>(value: T): T { return value; }
function count<T extends "a" | "b">(...values: T[]): number { return values.length; }
function record<T extends "a" | "b">(value: Record<string, T>): number { return Object.keys(value).length; }
function each<T extends Wide>(values: T[], read: (value: T) => number): number { return read(values[0]); }

function main(): void {
  const selected = choose({ kind: "wide", value: 1 });
  selected.value = 2;
  assert(selected.value === 2, "other union branches do not pin this branch's fields");
  const list = arrays([{ kind: "wide", value: 1 }]);
  list[0].value = 2;
  assert(list[0].value === 2, "union of array bounds selects the object's branch");
  const options = flags({ flag: true, count: 1, extra: "x" });
  const yes: true = options.flag;
  options.count = 2;
  options.extra = "y";
  assert(yes && options.count === 2 && options.extra === "y", "boolean context preserves literals without freezing other fields");
  const spreadValues: ("a" | "b")[] = ["a", "b"];
  assert(count(...spreadValues) === 2, "annotated spread values keep literal types");
  assert(record({ a: "a", b: "b" }) === 2, "index values keep constraint context");
  assert(each([{ kind: "wide", value: 1, extra: "x" }], value => value.extra.length) === 1, "callback inference preserves literals and extra fields");
}
