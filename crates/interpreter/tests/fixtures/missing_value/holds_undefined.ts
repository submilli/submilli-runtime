// SUB-2148: a read whose type already has room for `undefined` gives it for a
// missing element or key, and never throws.
function describe(x: unknown): string {
  return x === undefined ? "undefined" : String(x);
}

function main(): void {
  const data: Record<string, unknown> = { a: 1 };
  assert(describe(data["missing"]) === "undefined", "Record<string, unknown>");
  const id = data.sessionId;
  assert(typeof id !== "string", "an envelope field checked by typeof");
  const maybe: (string | undefined)[] = ["x"];
  const element = maybe[3];
  assert(element === undefined, "(string | undefined)[]");
  assert(describe(maybe[-1]) === "undefined", "a negative index");
}
