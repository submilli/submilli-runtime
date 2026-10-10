// Accessor halves inherit independently, but that must not soften the three
// rules around them: a data field and an accessor still cannot redeclare each
// other, and a property with only one half in the whole chain is still one-way.
//
// expect-error: `p` redeclares an inherited accessor as a field
// expect-error: `q` redeclares an inherited field as an accessor
// expect-error: cannot assign to read-only accessor `g` on `StillGetOnly`
// expect-error: property `s` is write-only (no getter)
class FieldOverAccessorBase {
  get p(): number {
    return 1;
  }
}
class FieldOverAccessor extends FieldOverAccessorBase {
  p: number = 2;
}

class AccessorOverFieldBase {
  q: number = 1;
}
class AccessorOverField extends AccessorOverFieldBase {
  get q(): number {
    return 2;
  }
}

class GetOnly {
  get g(): string {
    return "go";
  }
}
class StillGetOnly extends GetOnly {
  other(): number {
    return 1;
  }
}

class SetOnly {
  private v: string = "s";
  set s(x: string) {
    this.v = x;
  }
}
class StillSetOnly extends SetOnly {
  other(): number {
    return 1;
  }
}

export function main(): string {
  const a = new StillGetOnly();
  a.g = "no";
  const b = new StillSetOnly();
  return b.s;
}
