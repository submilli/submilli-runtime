// A type parameter in a union parameter takes the members of a union argument
// that the parameter's other members don't, as in tsc: `T | null` with a
// `Mode | null` argument binds `T` to `Mode`, as does an index signature's
// `A | number` with a `Mode` field. When the other members take every member,
// `T` takes the whole argument unless another argument decides it. A member
// of the same class or array kind pairs with that member, unless an identical
// one already did.
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
  assert(fromBox.length === 0 && fromField && agreeing[0] === "x" && viaCallback === 0, "other arguments");

  const inferredList = listOrNumbers(lists(true));
  const fromList: string[] | null = inferredList;
  const inferredPair = pairOrFlag(single(true));
  const fromPair: [number] | boolean | null = inferredPair;
  assert(fromList === null && fromPair === null, "paired members");
}
