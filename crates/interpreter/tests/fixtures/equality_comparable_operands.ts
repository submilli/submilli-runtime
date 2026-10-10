// `===`, `!==` and a `case` label accept operands whose types overlap, as
// tsc's comparable relation does, even when neither type is assignable to the
// other. The operands need not share a runtime representation either.
interface MaybeCount { count?: number }
interface MaybeLabel { count?: string }
interface Weak { toLowerCase?: () => string }
interface Derived { a: string; b: string }
interface Base { a: string }
class Point { x: number = 1; }
class Spot { x: number = 1; }
class Visible { x: number = 1; }
class Guarded { private secret: number = 1; x: number = 1; }
class Account { private id: number = 1; }
class Savings extends Account { rate: number = 1; }
class Checking extends Account { rate: number = 2; }
interface Chain<T> { value: T; wrap(): Chain<{ w: T }> }
interface Ping<T> { next: Pong<[T]>; wide: Pong<{ x: T }>; value: T }
interface Pong<T> { next: Ping<[T]>; wide: Ping<{ y: T }>; value: T }
class Tagged<T> { private tag: T; constructor(tag: T) { this.tag = tag; } }

function sameAs<T>(value: T, other: {}, n: number): boolean {
  return value === other || value === n || n === value || value === "text";
}

// Each step of the comparison meets a larger `Chain`; tsc assumes the
// pair related once the same named types recur.
function sameChain(left: Chain<{ a: 1; b: string }>, right: Chain<{ a: number; b: "x" }>): boolean {
  return left === right;
}

// Mutually recursive generics that grow at each level compare by their
// arguments, without walking every level.
function samePing(left: Ping<{ a: 1; b: string }>, right: Ping<{ a: number; b: "x" }>): boolean {
  return left === right;
}

function kind<T>(value: T): string {
  switch (value) {
    case 1:
      return "one";
    case "a":
      return "a";
    case true:
      return "true";
    case null:
      return "null";
    default:
      return "other";
  }
}

function describe(name: string): string {
  switch (name) {
    case null:
      return "missing";
    case "a":
      return "a";
    default:
      return "other";
  }
}

function main(): void {
  // Two optional members overlap on absence, whatever their types.
  const counted: MaybeCount = { count: 1 };
  const labelled: MaybeLabel = { count: "one" };
  assert(!(counted === labelled), "optional members of different types");
  assert(counted !== labelled, "optional members of different types, negated");

  // Each member may vary in its own direction.
  const literalFirst: { a: 1; b: string } = { a: 1, b: "x" };
  const literalSecond: { a: number; b: "a" } = { a: 2, b: "a" };
  assert(literalFirst !== literalSecond, "members vary independently");

  // A method's parameters compare in both directions.
  const handlerA: { fn(a: Derived, b: Base): void } = { fn: (a: Derived, b: Base) => {} };
  const handlerB: { fn(a: Base, b: Derived): void } = { fn: (a: Base, b: Derived) => {} };
  assert(handlerA !== handlerB, "method parameters are bivariant");

  // A tuple overlaps an array of one of its element types.
  const nums: number[] = [1];
  const pair: [number, string] = [1, "a"];
  assert(nums !== pair && pair !== nums, "tuple against array");

  // `{}` overlaps any value, including a primitive.
  const empty: {} = {};
  const n: number = 1;
  assert(n !== empty && empty !== 1 && "s" !== empty, "primitive against `{}`");

  // A literal overlaps a weak type that declares one of its members.
  const weak: Weak = {};
  assert(weak !== "A" && "A" !== weak, "literal against a weak type");

  // Classes of the same shape overlap; instances of different classes are
  // never equal (§1.6).
  assert(new Point() !== new Spot(), "same-shape classes");

  // Private members are compared by the class that declares them: a class
  // without the private member overlaps one that has it, and siblings share
  // their base's private member.
  assert(new Guarded() !== new Visible(), "private member against none");
  const savings = new Savings();
  const account: Account = savings;
  assert(savings === account, "subclass against its base");
  assert(savings !== new Checking(), "siblings sharing a private member");
  assert(new Tagged<1>(1) !== new Tagged<number>(2), "private member of overlapping instantiations");

  // An unconstrained type parameter overlaps anything.
  assert(sameAs(1, empty, 1), "type parameter against number");
  assert(sameAs("text", empty, 2), "type parameter against string literal");
  assert(!sameAs(true, empty, 2), "type parameter matching nothing");

  assert(kind(1) === "one" && kind("a") === "a" && kind(true) === "true", "mixed labels on `T`");
  assert(kind(null) === "null", "`case null` on `T`");
  assert(kind(2) === "other" && kind(false) === "other", "no label matches");

  // `null` is a valid label for a discriminant that can't hold it.
  assert(describe("a") === "a", "case after a null label");
  assert(describe("b") === "other", "default after a null label");
}
