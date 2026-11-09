// SUB-2190: the existing missing-read rules also decide missing JSON text.
type MaybeText = string | undefined;
let events = "";

function value(): unknown { events += "v"; return undefined; }
function replacer(): null { events += "r"; return null; }
function space(): number { events += "s"; return 2; }
function one(): number { return 1; }

function outcome(read: () => unknown): string {
  try {
    const value = read();
    return value === undefined ? "undefined" : String(value);
  } catch (error) {
    return error.name;
  }
}

function required(value: unknown): string {
  return JSON.stringify(value);
}

function optional(value: unknown): MaybeText {
  return JSON.stringify(value);
}

function erasedReturn(value: unknown): unknown {
  return JSON.stringify(value);
}

function generic<T>(value: T): string {
  return JSON.stringify(value);
}

function optionalArg(value?: string): boolean {
  return value === undefined;
}

function unknownArg(value?: unknown): boolean {
  return value === undefined;
}

function captured(): () => string {
  const text = JSON.stringify(undefined);
  return () => text;
}

function assigned(): string {
  let text = JSON.stringify(1);
  text = JSON.stringify(undefined);
  return text;
}

function sliceText(): string {
  return JSON.stringify(undefined).slice(0, 4);
}

function concatenateText(): string {
  return "text=" + JSON.stringify(undefined);
}

function main(): void {
  assert(outcome(() => required(undefined)) === "MissingValueError");
  assert(outcome(() => required(() => 1)) === "MissingValueError");
  assert(outcome(() => erasedReturn(undefined)) === "MissingValueError", "unknown does not name undefined");
  assert(outcome(() => generic(undefined)) === "MissingValueError");
  assert(outcome(() => generic(() => 1)) === "MissingValueError");
  assert(optional(undefined) === undefined);
  assert(optional(() => 1) === undefined);
  assert(generic(null) === "null");
  assert(generic(1) === "1");
  assert(generic({ value: 1 }) === '{"value":1}');

  assert(optionalArg(JSON.stringify(undefined)), "optional string argument accepts missing text");
  assert(outcome(() => unknownArg(JSON.stringify(undefined))) === "MissingValueError");
  const record: { text?: string } = { text: JSON.stringify(undefined) };
  assert(record.text === undefined, "optional field accepts missing text");
  let text: string | undefined = JSON.stringify(undefined);
  assert(text === undefined);
  text = JSON.stringify(one);
  assert(text === undefined, "an optional local accepts a later missing result");
  const capturedOutcome = outcome(captured);
  assert(capturedOutcome === "MissingValueError", "a captured local reads the value at initialization: " + capturedOutcome);
  assert(outcome(assigned) === "MissingValueError", "assignment does not silently lose a missing value");
  assert(outcome(sliceText) === "MissingValueError");
  assert(outcome(concatenateText) === "MissingValueError");

  assert(typeof JSON.stringify(undefined) === "undefined");
  assert(!JSON.stringify(undefined));
  assert((JSON.stringify(undefined) || "fallback") === "fallback");
  assert(JSON.stringify(undefined)?.length === undefined);
  assert((JSON.stringify(undefined) ?? "fallback") === "fallback");

  events = "";
  let missing = false;
  try {
    const serialized: string = JSON.stringify(value(), replacer(), space());
    assert(false, serialized);
  } catch (error) {
    assert(error.name === "MissingValueError");
    missing = true;
  }
  assert(missing);
  assert(events === "vrs", "optional arguments evaluate before the missing-result error");
  events = "";
  assert(JSON.stringify(value(), replacer(), space()) === undefined);
  assert(events === "vrs", "checks keep argument evaluation order");
  const args: [undefined, null, number] = [undefined, null, 2];
  assert(JSON.stringify(...args) === undefined, "spread call still reaches the check");
  missing = false;
  try {
    const serialized: string = JSON.stringify(...args);
    assert(false, serialized);
  } catch (error) {
    assert(error.name === "MissingValueError");
    missing = true;
  }
  assert(missing);
  assert(JSON.stringify({ n: 1 }, null, 2) === '{\n  "n": 1\n}');
}
