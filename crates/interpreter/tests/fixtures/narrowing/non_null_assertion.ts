interface Box {
  value: string | null;
}

function maybeString(present: boolean): string | null {
  if (present) {
    return "hello";
  }
  return null;
}

function maybeBox(present: boolean): Box | null {
  if (present) {
    return { value: "boxed" };
  }
  return null;
}

function main(): void {
  const text = maybeString(true);
  assert(text!.length === 5, "non-null assertion strips null from string");

  const plain = "already";
  assert(plain!.length === 7, "already non-null operand is accepted");

  const b = maybeBox(true);
  assert(b!.value! === "boxed", "chained assertions work on fields");

  const values: Array<string | null> = ["first", null];
  assert(values[0]!.length === 5, "index result can be asserted non-null");

  const fromCall = maybeString(true)!;
  assert(fromCall === "hello", "call result can be asserted non-null");

  assert("a" != "b", "!= still parses as binary");
  assert("a" !== "b", "!== still parses as binary");

  const absent = maybeString(false);
  let caught = false;
  try {
    absent!;
  } catch (e: Error) {
    caught = true;
    assert(e.name === "TypeError", "null assertion throws TypeError");
    assert(
      e.message === "non-null assertion failed: value is null",
      "null assertion preserves message",
    );
  }
  assert(caught, "null assertion throws catchable Error");

  let caughtLiteral = false;
  try {
    null!;
  } catch (e: TypeError) {
    caughtLiteral = true;
    assert(e.name === "TypeError", "literal null assertion throws TypeError");
  }
  assert(caughtLiteral, "literal null assertion is catchable as TypeError");
}
