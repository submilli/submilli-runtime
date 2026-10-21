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

// A padding default may be any expression: a literal, a function or an
// optional chain pads its slot as a plain value does.
function arrayDefault([a, b = [0]] = [1]): number {
  return a + b.length;
}
function arrowDefault({ x, f = () => 2 } = { x: 1 }): number {
  return x + f();
}
function objectDefault({ x, o = { k: 1 } } = { x: 1 }): number {
  return x + o.k;
}
const maybe: string | null = "s";
function chainDefault([a, b = maybe?.length ?? 0] = [1]): number {
  return a + b;
}

// A padding default that reads an earlier parameter reads that binding,
// not the outer one of the same name.
const first = "outer";
function readsEarlierParameter(first: number, [a, b = first] = [1]): number {
  return a + b;
}

// A closure in a padding default may read a later parameter; the slots
// before it still pad.
function readsLaterTuple([a, b = 2, g = () => later] = [1], later = "s"): number {
  return a + b + g().length;
}

// A closure parameter that shadows a pattern's name reads its own.
function shadowsPattern({ x, g = (x: number) => x * 2 } = { x: 1 }): number {
  return g(x);
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
  assert(arrayDefault([2, [1, 2]]) === 4, "an array literal default");
  assert(arrowDefault({ x: 1, f: () => 3 }) === 4, "an arrow default");
  assert(objectDefault({ x: 1, o: { k: 5 } }) === 6, "an object literal default");
  assert(chainDefault([1, 4]) === 5, "an optional chain default");
  assert(readsEarlierParameter(5) === 6, "a padding default reads an earlier parameter");
  assert(readsEarlierParameter(5, [1, 2]) === 3, "its slot takes a number");
  assert(first === "outer", "the outer name is untouched");
  assert(readsLaterTuple([1, 5]) === 7, "a slot before a later parameter's reader");
  assert(shadowsPattern({ x: 2, g: (n: number) => n + 1 }) === 3, "a shadowing closure");
  assert(shadowsPattern() === 2, "a shadowing closure as the default");
}
