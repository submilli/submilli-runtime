enum NumericKey { One = 1, Two = 2 }
enum TextKey { A = "a", B = "b" }
type IdKey = `id_${string}`;
function main(): void {
  checkCasts();
  checkTemplateBoundaries();
  checkEvaluationOrder();
  checkCompositions();
  const allStrings: `${string}${string}` = "";
  assert(allStrings === "", "unrestricted adjacent string holes collapse");
  const stringKey: keyof Record<string, boolean> = "key";
  const numericKey: keyof { [key: string]: boolean } = 1;
  assert(stringKey === "key" && numericKey === 1, "mapped and written keyof domains");
  const byId: Record<number, boolean> = {};
  byId[42] = true;
  byId[1.5] = false;
  byId[-0] = true;
  byId[1e21] = false;
  byId[NaN] = true;
  assert(byId["42"] === true, "numeric literal string reads");
  assert(byId?.["1.5"] === false, "optional numeric literal string reads");
  assert(byId["0"] === true, "negative zero property");
  assert(byId["1e+21"] === false, "exponential property");
  assert(byId["NaN"] === true, "NaN property");
  assert(Object.keys(byId).includes("42"), "Object.keys returns strings");
  assert(JSON.stringify(byId).includes('"42":true'), "JSON property name");
  const signature: { [id: number]: boolean } = {};
  signature[7] = true;
  assert(signature[7] === true, "number index signature");
  const mixed: Record<"a" | 1, boolean> = { a: true, 1: false };
  assert(mixed[1] === false && mixed.a, "finite mixed keys");
  const required: Record<number | "label", boolean> = { label: true };
  required[2] = false;
  assert(required.label, "required finite key alongside numeric domain");
  const numericEnum: Record<NumericKey, boolean> = { 1: true, 2: false };
  const textEnum: Record<TextKey, boolean> = { a: true, b: false };
  assert(numericEnum[NumericKey.One] === true && textEnum[TextKey.A] === true, "enum keys");
  const patterned: Record<IdKey, boolean> = {};
  patterned["id_"] = true;
  patterned["id_42"] = false;
  assert(patterned["id_"] === true, "empty template substitution");
  const patternSignature: { [key: `id_${string}`]: boolean } = {};
  patternSignature["id_x"] = true;
  assert(patternSignature["id_x"] === true, "pattern index signature");
}

function checkCasts(): void {
  const source: unknown = { "1": true, label: "text" };
  const numeric = source as Record<number, boolean>;
  assert(numeric[1] === true, "cast ignores unrelated names");
  const patternSource: unknown = { id_a: true, other: "text" };
  const pattern = patternSource as Record<`id_${string}`, boolean>;
  assert(pattern.id_a === true, "pattern cast ignores unrelated names");
  const finite: Record<`n_${1 | 2}`, boolean> = { n_1: true, n_2: false };
  assert(finite.n_1 && !finite.n_2, "finite templates require fields");
  const unionSignature: { [key: string | number]: boolean } = {};
  unionSignature[3] = true;
  unionSignature.a = false;
  assert(unionSignature["3"] === true && unionSignature.a === false, "union signature");
  const named: { [key: number]: boolean; label: string } = { label: "ok", "1": true };
  assert(named.label === "ok" && named[1] === true, "unrelated explicit field");
}

function checkTemplateBoundaries(): void {
  const numeric: Record<`n_${number}`, boolean> = {};
  numeric["n_0x10"] = true;
  numeric["n_ "] = true;
  assert(numeric["n_0x10"] === true && numeric["n_ "] === true, "numeric template conversion");
  const bigint: Record<`b_${bigint}`, boolean> = {};
  bigint["b_-0x10"] = true;
  assert(bigint["b_-0x10"] === true, "bigint template syntax");
  const literal: Record<`id_${"abc"}${number}`, boolean> = {};
  literal["id_abc12"] = true;
  assert(literal["id_abc12"] === true, "literal hole joins template text");
  const surrogate: Record<`id_${string}`, boolean> = {};
  surrogate["id_\ud800"] = true;
  const unknownValue: unknown = surrogate;
  const checked = unknownValue as Record<`id_${string}`, boolean>;
  assert(checked["id_\ud800"] === true, "pattern casts preserve lone surrogate keys");
}

let order = "";
function receiver(record: Record<number, number>): Record<number, number> { order += "r"; return record; }
function key(): number { order += "k"; return 1; }
function value(): number { order += "v"; return 3; }
function checkEvaluationOrder(): void {
  const record: Record<number, number> = {};
  receiver(record)[key()] = value();
  assert(order === "rkv" && record[1] === 3, "numeric property conversion evaluates once in order");
}

function checkCompositions(): void {
  const a: Record<`a${string}`, boolean> = { a1: true };
  const b: Record<`b${string}`, boolean> = { b1: false };
  const assigned = Object.assign(a, b);
  assert(assigned.a1 === true && assigned.b1 === false, "same-value assign preserves both domains");
  const mixed: Record<number | `id_${string}`, boolean> = {};
  mixed[1] = true;
  mixed.id_ok = false;
  assert(mixed[1] === true && mixed.id_ok === false, "union domains share their value type");
}
