// expect-error: `v` redeclares an inherited field as an accessor
// expect-error: `w` redeclares an inherited accessor as a field
// expect-error: `m` redeclares an inherited method as a field
// expect-error: `n` redeclares an inherited field as a method
// expect-error: `a` redeclares an inherited method as an accessor
// expect-error: `b` redeclares an inherited accessor as a method
// expect-error: `k` redeclares an inherited field as an accessor
// expect-error: `k` redeclares an inherited accessor as a field
// expect-error: `p` redeclares an inherited accessor as a field
// expect-error: `sv` redeclares an inherited field as an accessor
// expect-error: `pm` redeclares an inherited field as a method
// expect-error: `pv` redeclares an inherited field as an accessor
// expect-error: `gk` redeclares an inherited field as an accessor
// expect-error: `fv` redeclares an inherited field as an accessor
// A redeclaration shares the inherited member's payload slot or vtable slot, so
// it has to keep the inherited kind. Each direction fails differently if it
// doesn't: an accessor over a field never runs (the data slot wins every
// access), a field over an accessor appends a second, independent property under
// one name, and a field over a method leaves `c.m` and `p.m()` naming different
// members.

class FieldBase {
  v: string = "b";
  n: string = "b";
}

class AccessorOverField extends FieldBase {
  get v(): string {
    return "child";
  }
}

class MethodOverField extends FieldBase {
  n(): string {
    return "child";
  }
}

class AccessorBase {
  private backing: string = "b";
  get w(): string {
    return this.backing;
  }
  set w(x: string) {
    this.backing = x;
  }
  get b(): string {
    return this.backing;
  }
}

class FieldOverAccessor extends AccessorBase {
  w: string = "child";
}

class MethodOverAccessor extends AccessorBase {
  b(): string {
    return "child";
  }
}

class MethodBase {
  m(): string {
    return "b";
  }
  a(): string {
    return "b";
  }
}

class FieldOverMethod extends MethodBase {
  m: string = "child";
}

class AccessorOverMethod extends MethodBase {
  get a(): string {
    return "child";
  }
}

// The kind is decided by the *nearest* ancestor that declares the name, so a
// mid-chain change of kind is reported where it happens — and the leaf is then
// checked against the accessor, not against the grandparent's field.
class GrandParent {
  k: string = "gp";
}

class Middle extends GrandParent {
  get k(): string {
    return "mid";
  }
}

class Leaf extends Middle {
  k: number = 1;
}

// A parameter property declares a field, and reaches the same slot.
class ParamAccessorBase {
  get p(): string {
    return "base";
  }
}

class ParamPropOverAccessor extends ParamAccessorBase {
  constructor(public p: string) {
    super();
  }
}

// A `set`-only redeclaration of an inherited field is the same collision — the
// half a class declares does not change which kind it declares.
class SetterBase {
  sv: string = "b";
}

class SetterOverField extends SetterBase {
  set sv(x: string) {}
}

// A method over an inherited parameter property.
class ParamMethodBase {
  constructor(public pm: string) {}
}

class MethodOverParamProp extends ParamMethodBase {
  constructor() {
    super("p");
  }
  pm(): string {
    return "c";
  }
}

// The inherited field is `private`, so the collision is invisible in the source
// — and the shared slot is exactly why it has to be reported.
class PrivBase {
  private pv: string = "p";
  peek(): string {
    return this.pv;
  }
}

class AccOverPrivate extends PrivBase {
  get pv(): string {
    return "c";
  }
}

// A generic parent instantiated concretely.
class GKindBase<T> {
  gk: T;
  constructor(v: T) {
    this.gk = v;
  }
}

class AccOverGeneric extends GKindBase<string> {
  constructor() {
    super("p");
  }
  get gk(): string {
    return "acc";
  }
}

// The child is declared *before* its parent: the check runs after every class
// signature is bound, so source order does not decide what it can see.
class FwdChild extends FwdParent {
  get fv(): string {
    return "c";
  }
}

class FwdParent {
  fv: string = "p";
}

function main(): void {}
