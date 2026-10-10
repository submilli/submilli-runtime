// expect-error: accessor `get v` is not compatible with the inherited accessor
// expect-error: accessor `set w` is not compatible with the inherited accessor
// expect-error: accessor `get g` is not compatible with the inherited accessor
// expect-error: accessor `get w` is not compatible with the inherited accessor
// An accessor redeclaring an inherited one shares its vtable slot, whose
// signature the base declarer fixed. A getter must stay covariant in its return
// and a setter contravariant in its parameter, exactly as a method override is —
// otherwise the body doesn't fit the slot it is installed into.

class Base {
  private n: number = 1;
  get v(): number {
    return this.n;
  }
  get w(): string {
    return "b";
  }
  set w(x: string | null) {
    this.n = x === null ? 0 : 1;
  }
}

class BadGetter extends Base {
  get v(): string {
    return "child";
  }
}

// The inherited setter accepts `string | null`; this one would reject the null a
// parent-typed write is allowed to hand over. The getter half above it is fine,
// so the caret has to land on the setter — `get w`/`set w` are one property and
// one check, but each half reports at its own declaration.
class BadSetter extends Base {
  get w(): string {
    return "child";
  }
  set w(x: string) {
    this.q = x;
  }
  private q: string = "";
}

// Both halves wrong: two diagnostics, each on its own line.
class BothBad extends Base {
  get w(): number {
    return 1;
  }
  set w(x: string) {
    this.r = x;
  }
  private r: string = "";
}

class GenericBase<T> {
  private t: T;
  constructor(t: T) {
    this.t = t;
  }
  get g(): T {
    return this.t;
  }
}

// `U` is not known to satisfy `string`, so the getter is compared at opaque
// parameters rather than matching by name.
class BadGenericGetter<U> extends GenericBase<string> {
  private u: U;
  constructor(u: U) {
    super("parent");
    this.u = u;
  }
  get g(): U {
    return this.u;
  }
}

function main(): void {}
