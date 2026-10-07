// Inside a function, a narrowing on a module-level `let` survives a call that
// reassigns it, as in TypeScript, and the read after the call sees the value
// the call left, as in JavaScript: a member the value lacks throws a
// `TypeError`, and a value read whole is the new one.
let label: string | null = "a";
let value: string | number = "s";

function clear(): void {
  label = null;
}

function reset(): void {
  value = 5;
}

function guardThenCall(): number {
  if (label !== null) {
    clear();
    return label.length;
  }
  return 0;
}

function assignThenCall(): string {
  value = "abc";
  reset();
  return value + "!";
}

function main(): void {
  let caught = false;
  try {
    guardThenCall();
  } catch (error) {
    caught = error instanceof TypeError;
  }
  assert(caught, "reading a member of the null the call left throws");
  assert(assignThenCall() === "5!", "the read sees the number the call wrote");
  console.log("ok");
}
