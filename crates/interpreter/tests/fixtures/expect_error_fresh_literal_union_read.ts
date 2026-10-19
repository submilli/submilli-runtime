// expect-error: field `a` does not exist on all members
// expect-error-count: 13
// Reads the fresh-literal rule must not accept: a member that looks like the
// literal may hold another value. Each case says why.

// A declared member with the literal's shape may lack the field at run time.
function emptyDeclared(o: { a: number } | {} | undefined): unknown {
  return (o ?? {}).a;
}

function sameShapeDeclared(o: { a: number } | { b: number }, flag: boolean): unknown {
  return (flag ? o : { b: 1 }).a;
}

// `object` and a generic `T` fold into the literal's `{}`.
function objectDeclared(o: object | { a: number } | undefined): unknown {
  return (o || {}).a;
}

function genericDeclared<T>(t: T | undefined, u: { a: number } | undefined): unknown {
  return (t ?? u ?? {}).a;
}

// A spread copies whatever fields its source has.
function spreadLiteral(o: { c: number }, x: { b: number }, flag: boolean): unknown {
  return (flag ? o : { ...x }).c;
}

function spreadWithField(o: { c: number } | undefined, x: { b: number }): unknown {
  return (o || { ...x, d: 2 }).c;
}

// The join reduces `o` into the nested literals' `{ b: number }`.
function reducedDeclared(
  p: { c: boolean } | undefined,
  o: { b: number; c: string } | undefined,
  flag: boolean,
): unknown {
  return (p ?? (o || (flag ? { b: 1 } : { b: 2 }))).c;
}

// `y` folds into the literals while `x`, of the same shape, stays.
function reducedBesideSameShape(
  x: { a: string; c: string } | undefined,
  y: { a: number; c: number } | undefined,
  flag: boolean,
): unknown {
  return (x ?? (y ?? (flag ? { a: 1 } : { a: 2 }))).c;
}

// A literal of the folded operand's shape doesn't stand in for it.
function reducedBesideSameShapeLiteral(
  o: { b: number; c: number } | undefined,
  g: boolean,
  flag: boolean,
): unknown {
  return (g ? { b: "x", c: "y" } : o || (flag ? { b: 1 } : { b: 2 })).c;
}

// `??` folds the `{ f: string }` literal into the `{}` one.
function literalFoldedIntoLiteral(
  d: { f: number } | undefined,
  flag: boolean,
  g: boolean,
): unknown {
  return (d || ((flag ? { f: "s" } : undefined) ?? (g ? {} : undefined)) || {}).f;
}

// A literal inherits `valueOf` rather than lacking it.
function inheritedName(o: { valueOf: number } | undefined): unknown {
  return (o || {}).valueOf;
}

// An interface may hold a falsy primitive, which `&&` passes on.
interface Anything {}
function interfaceGuard(e: Anything | undefined): unknown {
  return ((e && { a: 1 }) || {}).b;
}

// `{}` may hold a falsy primitive, which `&&` passes on.
function emptyGuard(e: {}, flag: boolean): unknown {
  return (flag ? e && { a: 1 } : {}).a;
}

function main(): void {
  emptyDeclared(undefined);
  sameShapeDeclared({ b: 2 }, true);
  objectDeclared(undefined);
  genericDeclared<number>(undefined, undefined);
  spreadLiteral({ c: 1 }, { b: 1 }, false);
  spreadWithField(undefined, { b: 1 });
  reducedDeclared(undefined, undefined, true);
  reducedBesideSameShape(undefined, undefined, true);
  reducedBesideSameShapeLiteral(undefined, false, true);
  literalFoldedIntoLiteral(undefined, true, true);
  inheritedName(undefined);
  interfaceGuard(undefined);
  emptyGuard(1, true);
}
