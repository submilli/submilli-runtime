// `Error | null` unions lower through the shared object representation.
function firstError(fail: boolean): Error | null {
  if (fail) {
    return new Error("failed");
  }
  return null;
}

function main(): void {
  const none = firstError(false);
  assert(none === null, "null member flows through");
  const some = firstError(true);
  if (some !== null) {
    assert(some.message === "failed", "Error member readable after narrowing");
  } else {
    assert(false, "expected an Error");
  }
}
