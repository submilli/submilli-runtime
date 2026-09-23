// `true` and `false` are types, as in TypeScript. `boolean` is `true | false`.
// The common use is a result union tagged by a boolean field, which narrows on
// the tag's truthiness, on `===`, and in a `switch`.

type Ok = { ok: true; value: number };
type Failure = { ok: false; error: string };
type Result = Ok | Failure;

function parse(s: string): Result {
  const n = parseInt(s);
  if (isNaN(n)) {
    return { ok: false, error: `bad ${s}` };
  }
  return { ok: true, value: n };
}

function byTruthiness(r: Result): string {
  if (r.ok) {
    return `value ${r.value}`;
  }
  return `error ${r.error}`;
}

function byEquality(r: Result): string {
  if (r.ok === false) {
    return `error ${r.error}`;
  }
  return `value ${r.value}`;
}

function bySwitch(r: Result): string {
  switch (r.ok) {
    case true:
      return `value ${r.value}`;
    case false:
      return `error ${r.error}`;
  }
}

function describe(b: boolean): string {
  if (b === true) {
    const t: true = b;
    return `${t}`;
  }
  const f: false = b;
  return `${f}`;
}

function main(): void {
  assert(byTruthiness(parse("12")) === "value 12", "truthy tag narrows to Ok");
  assert(byTruthiness(parse("x")) === "error bad x", "falsy tag narrows to Failure");
  assert(byEquality(parse("3")) === "value 3", "=== narrows");
  assert(bySwitch(parse("z")) === "error bad z", "switch narrows and is exhaustive");
  assert(describe(true) === "true" && describe(false) === "false", "=== true on boolean");

  const t = true;
  const annotated: true = t;
  let widened = t;
  widened = false;
  assert(annotated && !widened, "a const keeps the literal type, a let widens");
  const both: true | false = widened;
  const plain: boolean = annotated;
  assert(!both && plain, "true | false is boolean");

  assert(JSON.stringify(parse("7")) === "{\"ok\":true,\"value\":7}", "stringify");
  const parsed = JSON.parse("{\"ok\":true,\"value\":4}") as Result;
  assert(byTruthiness(parsed) === "value 4", "JSON.parse into a tagged union");

  // A checked cast tests the value, not only that it is a boolean.
  const no: unknown = false;
  let threw = false;
  try {
    const yes = no as true;
    console.log(yes);
  } catch (e) {
    threw = true;
  }
  assert(threw, "`false as true` throws");
  let rejected = false;
  try {
    const wrongTag = JSON.parse("{\"ok\":true,\"error\":\"e\"}") as Result;
    console.log(wrongTag.ok);
  } catch (e) {
    rejected = true;
  }
  assert(rejected, "JSON.parse checks the tag against the variant's fields");
}
