// A type parameter in a union parameter takes the members of a union argument
// that the parameter's other members don't, as in tsc: `T | null` with a
// `Mode | null` argument binds `T` to `Mode`, as does an index signature's
// `A | number` with a `Mode` field. When the other members take every member,
// `T` takes the whole argument unless another argument decides it. A member
// of the same class or interface, or an array of the same mutability, pairs
// with that member, unless an identical one already did; when nothing else
// binds `T`, that member goes to `T` with the whole argument.
type Mode = "on" | "off";

function orElse<T>(value: T | null, fallback: T): T {
  return value === null ? fallback : value;
}

function unwrap<T>(value: T | null): T[] {
  return value === null ? [] : [value];
}

function lookup(key: number): Mode | null {
  return key > 0 ? "on" : null;
}

function lookupEither(key: number): string | number | null {
  return key > 0 ? 1 : null;
}

function firstNamed<A>(record: { [key: string]: A | number }): A | null {
  return null;
}

function textOrCount(key: number): string | number {
  return key > 0 ? "s" : 1;
}

function list<T>(value: T | string | number): T[] {
  return [];
}

function withLater<T>(value: T | string | number, later: T): T[] {
  return [later];
}

class Box<X> {
  constructor(public value: X) {}
}

function boxed<T>(value: T | Box<T> | null): T[] {
  return [];
}

function maybeBox(flag: boolean): Box<number> | null {
  return flag ? new Box(1) : null;
}

function inField<T>(value: T | string | number, holder: { k: T }): T {
  return holder.k;
}

function inCallback<T>(value: T | string | number, use: (each: T) => number): number {
  return 0;
}

function inObject<T>(holder: { value: T | string | number; use: (each: T) => number }): number {
  return 1;
}

function counted<T>(holder: { value: T | string | number; list: T[]; use: (each: T) => number }): number {
  return holder.list.map(holder.use).length;
}

function orBox<T>(value: T | Box<number> | null): T[] {
  return [];
}

function textBox(flag: boolean): Box<string> | null {
  return flag ? new Box("s") : null;
}

function boxedOrNumber<T>(value: T): T | Box<number> {
  return value;
}

function tagged<T>(holder: { value: T | string | number; list: T[]; last: T; use: (each: T) => number }): T[] {
  return holder.list;
}

function textsAndCounts(): (string | number)[] {
  return ["a", 1];
}

function annotatedEach<T>(use: (x: T | Box<number>, i: number) => number, value: T): T {
  use(new Box(5), 0);
  return value;
}

function eitherOf<T>(first: T | Box<number>, second: T | Box<number>): T | null {
  return first instanceof Box || second instanceof Box ? null : first;
}

function boxOrMark<T>(first: T | Box<number>, mark: T | "x"): T[] {
  return [];
}

function boxOrOther<T, U>(first: T | Box<number>, other: T | U): T[] {
  return [];
}

function textOrFlagBox(flag: boolean): Box<string> | Box<boolean> {
  return flag ? new Box("s") : new Box(true);
}

function listOrNumbers<T>(value: T | number[]): T | null {
  return null;
}

function lists(flag: boolean): string[] | number[] {
  return flag ? ["s"] : [1];
}

function pairOrFlag<T>(value: T | [number, number]): T | null {
  return null;
}

function single(flag: boolean): [number] | boolean {
  return flag ? [1] : false;
}

function main(): void {
  const found: Mode[] = unwrap(lookup(1));
  const chosen: Mode = orElse(lookup(0), "off");
  const either: (string | number)[] = unwrap(lookupEither(1));
  let mode: Mode = "on";
  if (found.length > 5) {
    mode = "off";
  }
  const named: Mode | null = firstNamed({ z: mode });
  assert(found.join(",") === "on" && chosen === "off", "a nullable union");
  assert(either.join(",") === "1" && named === null, "several members");

  const inferredWhole = list(textOrCount(1));
  const whole: (string | number)[] = inferredWhole;
  const inferredNullable = list(lookup(1));
  const nullable: (Mode | null)[] = inferredNullable;
  const inferredDecided = withLater(textOrCount(1), true);
  const decided: boolean[] = inferredDecided;
  assert(whole.length === 0 && nullable.length === 0 && decided[0], "every member taken");

  const inferredBoxed = boxed(maybeBox(true));
  const fromBox: number[] = inferredBoxed;
  const inferredField = inField(textOrCount(1), { k: true });
  const fromField: boolean = inferredField;
  const inferredAgreeing = withLater(textOrCount(1), "x");
  const agreeing: string[] = inferredAgreeing;
  const viaCallback = inCallback(textOrCount(1), (each) => (typeof each === "string" ? 1 : 2));
  const viaField = inObject({ value: textOrCount(1), use: (each) => (typeof each === "string" ? 1 : 2) });
  assert(fromBox.length === 0 && fromField && agreeing[0] === "x" && viaCallback === 0, "other arguments");
  const viaList = counted({ value: textOrCount(1), list: [true, false], use: (each) => (each ? 1 : 2) });
  const inferredBoxes = orBox(textBox(false));
  const boxes: (Box<string> | null)[] = inferredBoxes;
  assert(viaField === 1 && viaList === 2, "a callback beside the union");
  assert(boxes.length === 0, "a closely matched member the fallback takes");

  const expectedWider: Box<string> | Box<number | boolean> = boxedOrNumber(new Box("s"));
  const label = "x";
  const inferredTagged = tagged({ value: textOrCount(1), list: textsAndCounts(), last: label, use: (each) => 1 });
  const taggedList: (string | number)[] = inferredTagged;
  assert(expectedWider instanceof Box, "an expected result's close match is not checked");
  assert(taggedList.length === 2, "a non-callback field binds the fallback's type");

  const viaAnnotation = annotatedEach((x: Box<string> | Box<number | string>, i) => i, new Box("s"));
  assert(viaAnnotation.value === "s", "an annotated callback parameter wider than its slot");

  const narrowerLater = eitherOf(textOrFlagBox(true), new Box(true));
  const widerLater = eitherOf(new Box(true), textOrFlagBox(false));
  const bothBoxes: Box<string> | Box<boolean> | null = narrowerLater ?? widerLater;
  assert(bothBoxes === null, "two union slots take the wider of their arguments");

  const exactLater = eitherOf(new Box(true), new Box(1));
  const flagBox: Box<boolean> | null = exactLater;
  assert(flagBox === null, "a later argument its own member takes leaves the fallback");

  const marked = boxOrMark(new Box(true), "x");
  marked.push(new Box(false));
  assert(marked.length === 1, "a literal its literal member takes leaves the fallback");

  const others = boxOrOther(new Box(true), new Box("s"));
  others.push(new Box(false));
  assert(others.length === 1, "another type parameter takes a later argument beside the fallback");

  const inferredList = listOrNumbers(lists(true));
  const fromList: string[] | null = inferredList;
  const inferredPair = pairOrFlag(single(true));
  const fromPair: [number] | boolean | null = inferredPair;
  assert(fromList === null && fromPair === null, "paired members");
}
