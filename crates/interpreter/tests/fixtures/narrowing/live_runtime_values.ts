let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function arithmetic(): number {
  if (current === null || clear()) { return 0; }
  else { return current + 1; }
}

interface ChangingValue { readonly value: string | null; }
class GetterValue implements ChangingValue {
  private reads: number = 0;
  get value(): string | null {
    this.reads += 1;
    return this.reads === 1 ? "first" : null;
  }
}
function getterResult(box: ChangingValue): string {
  if (box.value !== null) { return box.value; }
  return "fallback";
}
function replace(values: unknown[]): void { values[0] = 3; }
function elementResult(values: unknown[]): string {
  if (values[0] === "a") { replace(values); return values[0]; }
  return "fallback";
}
function identity(value: string): string { return value; }
function computedWrite(index: number): 3 {
  const values: number[] = [3];
  if (values[0] === 3) {
    values[index] = 4;
    return values[0];
  }
  return 3;
}
function expectFour(value: unknown): void { assert(value === 4); }
function generic<T>(value: T): T { return value; }
class Relay {
  relay(value: string): string { return value; }
  read(box: ChangingValue): string { return getterResult(box); }
}
function closureResult(): unknown {
  const value: string = getterResult(new GetterValue());
  const read = (): string => value;
  return read();
}
function objectResult(): unknown {
  const value = { text: getterResult(new GetterValue()) };
  return value.text;
}
function arrayResult(): unknown {
  const values: string[] = [getterResult(new GetterValue())];
  return values[0];
}
let nullableValues: number[] | null = [1];
let indexCalls = 0;
function clearValues(): boolean { nullableValues = null; return true; }
function nextIndex(): number { indexCalls += 1; return 0; }
function checkIndexOrder(): void {
  let caught = false;
  try {
    if (nullableValues !== null && clearValues()) { nullableValues[nextIndex()]; }
  } catch (error) { caught = error instanceof TypeError; }
  assert(caught);
  assert(indexCalls === 1);
}
function main(): void {
  assert(arithmetic() === 1);
  const getter: unknown = identity(getterResult(new GetterValue()));
  assert(getter === null);
  const element: unknown = identity(elementResult(["a"]));
  assert(element === 3);
  expectFour(computedWrite(0));
  assert(generic(getterResult(new GetterValue())) === null);
  const relay = new Relay();
  const method: unknown = relay.relay(relay.read(new GetterValue()));
  assert(method === null);
  assert(closureResult() === null);
  assert(objectResult() === null);
  assert(arrayResult() === null);
  checkIndexOrder();
}
