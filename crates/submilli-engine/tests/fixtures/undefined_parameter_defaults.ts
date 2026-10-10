let trace: string = "";
const seed: number = 7;

function mark(label: string, value: number): number {
  trace += label;
  return value;
}

function defaults(first: number = mark("a", seed), second: number = mark("b", first + 1)): number {
  return first + second;
}

function beforeRequired(first: number = 4, second: number): number {
  return first + second;
}

function nullable(value: number | null = 3): number | null {
  return value;
}

function optional(value?: number): number | undefined {
  return value;
}

function captured(read: () => number = () => value, value: number = 5): number {
  value += 1;
  return read();
}

class Counter {
  value: number;
  constructor(value: number = seed) { this.value = value; }
  add(value: number = this.value): number { return this.value + value; }
  static pick(value: number = seed): number { return value; }
}

function main(): void {
  assert(defaults() === 15, "omitted parameters initialize in declaration order");
  assert(trace === "ab", "each default runs once");
  trace = "";
  assert(defaults(mark("x", 2), mark("y", 3)) === 5, "explicit values retained");
  assert(trace === "xy", "arguments evaluate before defaults and suppress defaults");
  trace = "";
  assert(defaults(undefined, mark("y", 10)) === 17, "explicit undefined invokes default");
  assert(trace === "ya", "all call arguments evaluate before callee defaults");
  assert(beforeRequired(undefined, 2) === 6, "default before required parameter");
  assert(nullable(null) === null, "null does not invoke the default");
  assert(optional() === undefined, "optional parameter receives undefined");
  assert(captured() === 6, "default closure shares the later parameter's storage");
  const arrow = (value: number = seed): number => value;
  assert(arrow(undefined) === 7, "arrow default");
  function nested(value: number = seed): number { return value; }
  assert(nested() === 7, "nested default uses declaration scope");
  const counter = new Counter();
  assert(counter.value === 7, "constructor default");
  assert(counter.add() === 14, "method default may use this");
  assert(Counter.pick() === 7, "static method default");
}
