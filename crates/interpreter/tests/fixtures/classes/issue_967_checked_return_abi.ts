let invocations = 0;
function noop(value: number = 1): void { invocations += value; }
function text(): string { invocations += 1; return "text"; }

function checkReturnedValue(value: unknown, actualType: string): void {
  let caught = false;
  let castCompleted = false;
  let callCompleted = false;
  let returnCheckCompleted = false;
  try {
    const fn = value as () => number;
    castCompleted = true;
    const returned: unknown = fn();
    callCompleted = true;
    returned as number;
    returnCheckCompleted = true;
  } catch (error) {
    assert(error instanceof TypeError, "return mismatch is catchable, not a Wasm trap");
    assert(error.message === "type mismatch: expected number, got " + actualType);
    caught = true;
  }
  assert(castCompleted && callCompleted, "compatible boxed-result ABI permits invocation");
  assert(caught && !returnCheckCompleted, "the returned value still receives its explicit checked cast");
}

function main(): void {
  checkReturnedValue(noop, "undefined");
  checkReturnedValue(text, "string");
  assert(invocations === 2, "bodies run before their returned values are checked");
  const callback: () => void = (): number => 42;
  const erased: unknown = callback;
  const numeric = erased as () => number;
  assert((numeric() as number) === 42, "void annotations preserve actual return values");
}
