function objectValue(read: () => number = () => later, { later = 42 }: { later?: number } = {}): number {
  later = 7;
  return read();
}

function arrayValue(read: () => number = () => later, [later = 42]: [number?] = []): number {
  later = 8;
  return read();
}

function objectRest(read: () => number = () => tail.value, { head, ...tail }: { head: number; value: number } = { head: 0, value: 42 }): number {
  tail = { value: 9 };
  return read();
}

function arrayRest(read: () => number = () => tail[0], [head, ...tail]: number[] = [0, 42]): number {
  tail = [10];
  return read();
}

function capturedWrite(write: () => void = () => { later = 11; }, read: () => number = () => later, { later = 42 }: { later?: number } = {}): number {
  write();
  return read();
}

function capturedRestWrite(write: () => void = () => { tail = { value: 12 }; }, read: () => number = () => tail.value, { head, ...tail }: { head: number; value: number } = { head: 0, value: 42 }): number {
  write();
  return read();
}

function earlyWrite(write: () => number = () => { later = 13; return later; }, result: number = write(), { later = 42 }: { later?: number } = {}): number {
  return result;
}

function earlyRestWrite(write: () => number = () => { tail = { value: 14 }; return tail.value; }, result: number = write(), { head, ...tail }: { head: number; value: number } = { head: 0, value: 42 }): number {
  return result;
}

function main(): void {
  assert(objectValue() === 7 && objectValue(undefined, { later: 100 }) === 7, "captured object parameter sees body writes");
  assert(arrayValue() === 8, "captured array parameter sees body writes");
  assert(objectRest() === 9 && arrayRest() === 10, "captured rest parameters see replacement values");
  assert(capturedWrite() === 11 && capturedRestWrite() === 12, "closures write shared parameter bindings");
  const arrow = (read: () => number = () => later, { later = 42 }: { later?: number } = {}): number => {
    later = 15;
    return read();
  };
  assert(arrow() === 15, "arrow parameter patterns are mutable");
  let caught = false;
  try { earlyWrite(); } catch (error: ReferenceError) { caught = true; }
  assert(caught, "a captured write before pattern initialization throws");
  caught = false;
  try { earlyRestWrite(); } catch (error: ReferenceError) { caught = true; }
  assert(caught, "a captured write before rest initialization throws");
}
