function optionalValue(): number | undefined { return undefined; }

function readLater(read: () => number | undefined = () => value, value = optionalValue()): number | undefined {
  return read();
}

function writeLater(write: () => void = () => { value = undefined; }, read: () => number | undefined = () => value, value = optionalValue()): number | undefined {
  write();
  return read();
}

function definiteLater(read: () => number = () => value, value = 7): number {
  return read();
}

function patternLater(read: () => number | undefined = () => value, { value = optionalValue() }: { value?: number } = {}): number | undefined {
  return read();
}

class LaterValue {
  result: number | undefined;
  constructor(read: () => number | undefined = () => value, value = optionalValue()) {
    this.result = read();
  }
  read(read: () => number | undefined = () => value, value = optionalValue()): number | undefined {
    return read();
  }
  throughThis(read: () => number | undefined = () => value, value: number | undefined = this.result): number | undefined {
    return read();
  }
}

function main(): void {
  assert(readLater() === undefined && readLater(undefined, 9) === 9);
  assert(writeLater(undefined, undefined, 10) === undefined, "captured writes use the same inferred source type");
  assert(definiteLater() === 7, "a definite initializer stays a definite parameter");
  assert(patternLater() === undefined && patternLater(undefined, { value: 11 }) === 11);
  const arrow = (read: () => number | undefined = () => value, value = optionalValue()): number | undefined => read();
  const expression = function(read: () => number | undefined = () => value, value = optionalValue()): number | undefined { return read(); };
  assert(arrow() === undefined && arrow(undefined, 12) === 12);
  assert(expression() === undefined && expression(undefined, 13) === 13);
  const instance = new LaterValue();
  assert(instance.result === undefined && new LaterValue(undefined, 14).result === 14);
  assert(instance.read() === undefined && instance.read(undefined, 15) === 15);
  assert(instance.throughThis() === undefined, "this-based defaults keep the same binding metadata");
}
