// expect-error: field `v` is not compatible with the inherited declaration
// expect-error: field `w` is not compatible with the inherited declaration
// expect-error: field `p` is not compatible with the inherited declaration
// expect-error: field `g` is not compatible with the inherited declaration
// expect-error: field `o` is not compatible with the inherited declaration
// expect-error: field `h` is not compatible with the inherited declaration
// A redeclared field shares the inherited field's payload slot, so a
// parent-typed receiver reads the child's value through the *parent's* declared
// type. Sound only when the child's type still satisfies the parent's.

class Base {
  v: string = "b";
  w: string = "b";
  p: string = "b";
  o: string = "b";
}

class Mistyped extends Base {
  v: number = 1;
}

class Widened extends Base {
  w: string | null = null;
}

// A parameter property declares a field too, and codegen shadows it the same
// way — so it needs the same check.
class ParamProp extends Base {
  constructor(public p: number) {
    super();
  }
}

// Optional widens the read type to `T | null`, which the parent doesn't admit.
class MadeOptional extends Base {
  o?: string;
}

class GenericBase<T> {
  g: T;
  h: T;
  constructor(g: T) {
    this.g = g;
    this.h = g;
  }
}

class Concrete {
  m(): string {
    return "c";
  }
}

// The parent's `h: T` must be compared at the *same* opaque parameter the child
// stands at, not left as a bare `TypeVar` — a `TypeVar` on either side makes
// `assignable` answer `true` and this shadow would silently reach codegen.
class PassthroughShadow<T> extends GenericBase<T> {
  h: Concrete = new Concrete();
  constructor(g: T) {
    super(g);
  }
}

// The child's own type parameter is unbound: `U` cannot be known to satisfy
// `string`. Compared as an opaque generic param, not as a wildcard `TypeVar`.
class UnboundParam<U> extends GenericBase<string> {
  g: U;
  constructor(g: U) {
    super("parent");
    this.g = g;
  }
}

function main(): void {}
