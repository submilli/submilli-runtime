interface Opts {
  unit: string;
}

// The vtable slot belongs to `Base`, so its result type is `(ref $ObjectShape)`.
// The override's returned value is interface-typed, which lowers to the wider
// `(ref null $Object)` — the return-side twin of the argument coercion.
class Base {
  make(): { unit: string } {
    return { unit: "base" };
  }
}

class Derived extends Base {
  make(): Opts {
    const o: Opts = { unit: "derived" };
    return o;
  }
}

// Nullable slot, and the override drops the `| null`.
class NullableBase {
  make(): { unit: string } | null {
    return { unit: "nullable-base" };
  }
}

class NullableDerived extends NullableBase {
  make(): { unit: string } {
    const o: Opts = { unit: "nullable-derived" };
    return o;
  }
}

// Three levels deep, with the middle link reached through `super`.
class Deep extends Derived {
  make(): Opts {
    const parent = super.make();
    return { unit: parent.unit + "-deep" };
  }
}

function unitOf(b: Base): string {
  return b.make().unit;
}

function nullableUnitOf(b: NullableBase): string {
  const made = b.make();
  return made === null ? "none" : made.unit;
}

function main(): void {
  assert(unitOf(new Base()) === "base", "base returns through its own slot");
  assert(unitOf(new Derived()) === "derived", "override returns into a narrower slot");
  assert(unitOf(new Deep()) === "derived-deep", "super call through the same slot");
  assert(
    nullableUnitOf(new NullableBase()) === "nullable-base",
    "nullable slot keeps its own return",
  );
  assert(
    nullableUnitOf(new NullableDerived()) === "nullable-derived",
    "override drops the null and still fits the slot",
  );
}
