// `asserts x is T`: after the call, the argument is `T`.
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function assertIsNumber(x: string | number): asserts x is number {
  if (typeof x !== "number") throw new Error("not a number");
  return;
}

function shout(v: unknown): string {
  assertIsString(v);
  return v.toUpperCase();
}

function main(): void {
  const v: unknown = "hi";
  assertIsString(v);
  assert(v.toUpperCase() === "HI", "narrowed after the call");
  assert(shout("ok") === "OK", "narrowed a parameter");

  let n: string | number = 2;
  assertIsNumber(n);
  assert(n + 1 === 3, "narrowed a union");

  let threw = false;
  try {
    shout(1);
  } catch (e) {
    threw = true;
  }
  assert(threw, "the assertion throws for a wrong value");
}
