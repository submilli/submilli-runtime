// A parameter's literal default is typed from its pattern, as in TypeScript:
// each defaulted slot or property the default lacks becomes optional in the
// parameter's type, so a caller may pass it.
function next(): number {
  return 3;
}

function offset({ x, y = 0 } = { x: 0 }): number {
  return x + y;
}

function origin({ x = 0, y = 0 } = {}): number {
  return x + y;
}

function negated([a, b = -1] = [1]): number {
  return a + b;
}

function computed([a, b = Math.max(1, 2)] = [1]): number {
  return a + b;
}

function called([a, b = next()] = [1]): number {
  return a + b;
}

function skipped([a, , c = 5] = [1]): number {
  return a + c;
}

function parenthesized([a, b = 5] = ([1])): number {
  return a + b;
}

// A padding default that reads a name the pattern binds reads the
// pattern's own binding, not the outer one.
const first = 10;
function readsPattern([first, second = first]: [number, number?] = [1]): number {
  return second;
}
function readsObjectPattern({ x = 3, y = x }: { x?: number; y?: number } = {}): number {
  return y;
}

function main(): void {
  assert(offset() === 0, "the default");
  assert(offset({ x: 1 }) === 1, "the property's default");
  assert(offset({ x: 1, y: 1 }) === 2, "a property the default lacks");

  assert(origin() === 0, "an empty default");
  assert(origin({}) === 0, "an empty argument");
  assert(origin({ y: 2 }) === 2, "one property");
  assert(origin({ x: 1, y: 2 }) === 3, "both properties");

  assert(negated() === 0, "a negated default");
  assert(negated([1, 2]) === 3, "a slot past a negated default");
  assert(computed([1, 2]) === 3, "a slot past a computed default");
  assert(called([1, 2]) === 3, "a slot past a call default");
  assert(called() === 4, "a call default runs");
  assert(skipped([1, 2, 3]) === 4, "a slot past a hole");
  assert(skipped() === 6, "a hole reads nothing");
  assert(parenthesized([1, 2]) === 3, "a parenthesized default");
  assert(readsPattern() === 1, "a padding default reads its pattern");
  assert(readsObjectPattern() === 3, "an object padding default reads its pattern");
  assert(first === 10, "the outer name is untouched");
}
