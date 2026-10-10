// Spreading a value whose type is a union that includes an index-signature
// type copies what the value holds. A dictionary member can hold a field
// another member names, with the dictionary's value type, so that field is
// copied too rather than dropped as a mismatch.

interface Dict {
  [key: string]: number;
}

interface Named {
  n: number;
  [key: string]: number | string;
}

interface Table<T> {
  [key: string]: T;
}

function show(label: string, actual: string, expected: string): void {
  console.log(label, actual);
  assert(actual === expected, label);
}

// Each union reaches its spread through a parameter, so flow narrowing
// can't reduce it to the one type of an initializer.
function copy(x: Dict | { a: string }): string {
  return JSON.stringify({ ...x, z: 1 });
}

function copyRecord(x: Record<string, number> | { a: string }): string {
  return JSON.stringify({ ...x });
}

function copyNamed(x: Named | { a: number; n: string }): string {
  const copied = { ...x };
  return `${String(copied.n)} ${String(copied.a)}`;
}

function copyTable(x: Table<boolean> | { a: number }): string {
  return JSON.stringify({ ...x });
}

function chosen(flag: boolean, x: Dict | { a: string }, y: Table<string>): string {
  return JSON.stringify({ ...(flag ? x : y) });
}

function threeWays(x: Dict | Table<string> | { a: boolean }): string {
  return JSON.stringify({ ...x });
}

function laterField(x: Dict | { a: string }): string {
  return JSON.stringify({ ...x, b: "late" });
}

function earlierField(x: Dict | { a: string }): string {
  const copied = { b: "first", ...x };
  return String(copied.b);
}

function main(): void {
  const dict: Dict = { a: 7, b: 8 };
  show("dictionary", copy(dict), '{"a":7,"b":8,"z":1}');
  show("object", copy({ a: "s" }), '{"a":"s","z":1}');
  show("empty dictionary", copy({}), '{"z":1}');

  const record: Record<string, number> = { a: 5 };
  show("record", copyRecord(record), '{"a":5}');
  show("record object", copyRecord({ a: "r" }), '{"a":"r"}');

  const named: Named = { n: 1, a: "q" };
  show("named field", copyNamed(named), "1 q");

  const flags: Table<boolean> = { a: true };
  show("generic", copyTable(flags), '{"a":true}');

  const strings: Table<string> = { a: "t" };
  show("conditional dictionary", chosen(true, dict, strings), '{"a":7,"b":8}');
  show("conditional other", chosen(false, dict, strings), '{"a":"t"}');
  show("two dictionaries", threeWays(strings), '{"a":"t"}');
  show("two dictionaries number", threeWays({ a: 3 } as Dict), '{"a":3}');
  show("two dictionaries object", threeWays({ a: false }), '{"a":false}');

  show("later field", laterField(dict), '{"a":7,"b":"late"}');
  show("earlier field", earlierField(dict), "8");
  show("earlier field kept", earlierField({ a: "s" }), "first");

  const both: (Dict | { a: string })[] = [dict, { a: "z" }];
  show(
    "in a callback",
    JSON.stringify(both.map((x) => ({ ...x, k: 0 }))),
    '[{"a":7,"b":8,"k":0},{"a":"z","k":0}]',
  );
}
