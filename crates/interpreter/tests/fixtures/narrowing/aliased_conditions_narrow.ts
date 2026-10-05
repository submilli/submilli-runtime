// Testing an unannotated `const` tests its initializer, as in TypeScript:
// a stored condition, a stored discriminant, or one destructured from the
// value narrows the value it was read from, when that value is a constant
// reference (a `const`, a parameter or `let` never assigned, or a `readonly`
// member of one).
type Shape = { kind: "circle"; radius: number } | { kind: "square"; side: number };
type Data = { kind: "str"; payload: string } | { kind: "num"; payload: number };

function storedTypeof(x: string | number): number {
  const isString = typeof x === "string";
  if (isString) {
    return x.length;
  }
  return x;
}

function storedDisjunction(x: string | number | boolean): string {
  const isString = typeof x === "string";
  const isNumber = typeof x === "number";
  const isStringOrNumber = isString || isNumber;
  if (isStringOrNumber) {
    const t: string | number = x;
    return typeof t === "string" ? t : "number";
  }
  const b: boolean = x;
  return b ? "yes" : "no";
}

function storedNullCheck(x: number | null): number {
  const present = x !== null;
  return present ? x : 0;
}

function storedDiscriminant(shape: Shape): number {
  const isCircle = shape.kind === "circle";
  if (isCircle) {
    return shape.radius;
  }
  return shape.side;
}

function letNeverAssigned(arg: Shape): number {
  let shape = arg;
  const isCircle = shape.kind === "circle";
  return isCircle ? shape.radius : shape.side;
}

function readonlyField(box: { readonly value: string | number }): number {
  const isString = typeof box.value === "string";
  if (isString) {
    return box.value.length;
  }
  return 0;
}

function readonlyTuple(pair: readonly [string | number]): number {
  const isString = typeof pair[0] === "string";
  if (isString) {
    return pair[0].length;
  }
  return 0;
}

function aliasedKind(shape: Shape): number {
  const kind = shape.kind;
  if (kind === "circle") {
    return shape.radius;
  }
  return shape.side;
}

function destructuredKind(shape: Shape): number {
  const { kind: k } = shape;
  switch (k) {
    case "circle":
      return shape.radius;
    case "square":
      return shape.side;
  }
}

function destructuredSibling(data: Data): number {
  const { kind, payload } = data;
  if (kind === "str") {
    return payload.length;
  }
  return payload;
}

function destructuredParameter({ kind, payload }: Data): number {
  if (kind === "num") {
    return payload;
  }
  return payload.length;
}

function makeData(): Data {
  return { kind: "str", payload: "four" };
}

const { kind: moduleKind, payload: modulePayload } = makeData();
const moduleSize = moduleKind === "str" ? modulePayload.length : modulePayload;

class Holder {
  readonly value: string | number;
  constructor(value: string | number) {
    this.value = value;
    const isString = typeof this.value === "string";
    if (isString) {
      const s: string = this.value;
      assert(s.length >= 0, "a readonly field of `this` narrows");
    }
  }
}

function main(): void {
  assert(storedTypeof("abc") === 3 && storedTypeof(4) === 4, "a stored `typeof`");
  assert(storedDisjunction("a") === "a" && storedDisjunction(2) === "number", "stored conditions combine");
  assert(storedDisjunction(true) === "yes", "and their false branch narrows");
  assert(storedNullCheck(5) === 5 && storedNullCheck(null) === 0, "a stored null check");
  const circle: Shape = { kind: "circle", radius: 2 };
  const square: Shape = { kind: "square", side: 3 };
  assert(storedDiscriminant(circle) === 2 && storedDiscriminant(square) === 3, "a stored discriminant test");
  assert(letNeverAssigned(circle) === 2 && letNeverAssigned(square) === 3, "a `let` never assigned");
  assert(readonlyField({ value: "ab" }) === 2 && readonlyField({ value: 1 }) === 0, "a readonly field");
  assert(readonlyTuple(["abc"]) === 3 && readonlyTuple([1]) === 0, "a readonly tuple element");
  assert(aliasedKind(circle) === 2 && aliasedKind(square) === 3, "a stored discriminant");
  assert(destructuredKind(circle) === 2 && destructuredKind(square) === 3, "a destructured discriminant");
  assert(destructuredSibling({ kind: "str", payload: "xyz" }) === 3, "a destructured sibling");
  assert(destructuredSibling({ kind: "num", payload: 7 }) === 7, "in both branches");
  assert(destructuredParameter({ kind: "num", payload: 8 }) === 8, "a destructured parameter");
  assert(destructuredParameter({ kind: "str", payload: "ab" }) === 2, "in both branches");
  assert(moduleSize === 4, "a module-level destructured sibling");
  const holder = new Holder("q");
  assert(holder.value === "q", "the holder keeps its value");
}
