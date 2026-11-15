// expect-error-count: 1
// expect-error: narrowing on `label` was dropped because a call may run a function
// The read after clear() must re-check the nullable module binding.
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
