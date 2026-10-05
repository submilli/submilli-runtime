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

function copy(x: Dict | { a: string }): string {
  return JSON.stringify({ ...x, z: 1 });
}

function main(): void {
  const dict: Dict = { a: 7, b: 8 };
  show("dictionary", copy(dict), '{"a":7,"b":8,"z":1}');
  show("object", copy({ a: "s" }), '{"a":"s","z":1}');
  show("empty dictionary", copy({}), '{"z":1}');

  const record: Record<string, number> = { a: 5 };
  const fromRecord: Record<string, number> | { a: string } = record;
  show("record", JSON.stringify({ ...fromRecord }), '{"a":5}');

  const named: Named = { n: 1, a: "q" };
  const fromNamed: Named | { a: number; n: string } = named;
  const namedCopy = { ...fromNamed };
  show("named field", `${namedCopy.n} ${String(namedCopy.a)}`, "1 q");

  const flags: Table<boolean> = { a: true };
  const fromTable: Table<boolean> | { a: number } = flags;
  show("generic", JSON.stringify({ ...fromTable }), '{"a":true}');

  const late: Dict | { a: string } = dict;
  show("later field", JSON.stringify({ ...late, b: "late" }), '{"a":7,"b":"late"}');
  show("earlier field", JSON.stringify({ a: "first", ...late }), '{"a":7,"b":8}');

  const both: (Dict | { a: string })[] = [dict, { a: "z" }];
  show(
    "in a callback",
    JSON.stringify(both.map((x) => ({ ...x, k: 0 }))),
    '[{"a":7,"b":8,"k":0},{"a":"z","k":0}]',
  );
}
