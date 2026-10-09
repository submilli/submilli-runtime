function earlyRead(
  read: () => number = () => value,
  result: number = read(),
  value: number = 5,
): number { return result; }

function earlyRequired(
  read: () => number = () => value,
  result: number = read(),
  value: number,
): number { return result; }

let effects = 0;
function writeValue(): number { effects += 1; return 42; }
function earlyWrite(
  write: () => number = () => { value = writeValue(); return value; },
  result: number = write(),
  value: number = 5,
): number { return result; }

function earlyPostfix(
  write: () => number = () => value++,
  result: number = write(),
  value: number = 5,
): number { return result; }

function deferred(read: () => number = () => value, value: number = 5): number {
  assert(read() === value, "initialized capture observes parameter");
  value = value + 1;
  return read();
}

class Receiver {
  read(read: () => number = () => value, result: number = read(), value: number = 5): number {
    return result;
  }
}

function main(): void {
  let errors = 0;
  try { earlyRead(undefined, undefined, 9); } catch (error) { assert(error instanceof ReferenceError); errors += 1; }
  try { earlyRequired(undefined, undefined, 9); } catch (error) { assert(error instanceof ReferenceError); errors += 1; }
  try { earlyWrite(undefined, undefined, 9); } catch (error) { assert(error instanceof ReferenceError); errors += 1; }
  try { earlyPostfix(undefined, undefined, 9); } catch (error) { assert(error instanceof ReferenceError); errors += 1; }
  const arrow = (read: () => number = () => value, result: number = read(), value: number = 5): number => result;
  try { arrow(undefined, undefined, 9); } catch (error) { assert(error instanceof ReferenceError); errors += 1; }
  try { new Receiver().read(undefined, undefined, 9); } catch (error) { assert(error instanceof ReferenceError); errors += 1; }
  assert(errors === 6, "early reads and writes reject initialized-looking incoming arguments");
  assert(effects === 1, "assignment evaluates its right-side effects before TDZ rejection");
  assert(deferred() === 6 && deferred(undefined, 9) === 10, "later initialized captures remain live");
}
