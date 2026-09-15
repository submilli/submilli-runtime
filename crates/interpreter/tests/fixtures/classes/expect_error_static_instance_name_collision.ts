// expect-error: method `m` must be called
// expect-error: method `pick` must be called
// expect-error: `only` is a static member of `Only` — access it on the class
// expect-error: `only` is a static member of `Only` — access it on the class
// A class may declare a static and an instance member of the same name
// (spec.md §Classes). Instance dispatch resolves the name to the *instance*
// member, so a naming diagnostic on an instance receiver has to report that one
// too — pointing at `C.m` would name a different function with a different
// signature than the `c.m(5)` that just compiled. The plain and `?.` paths
// resolve in the same order.

class C {
  static m(): number {
    return 1;
  }
  m(a: number): number {
    return a + 10;
  }
}

class Picker {
  static pick(): string {
    return "static";
  }
  pick(a: string): string {
    return a;
  }
}

// A name that really is static-only still gets the static message, plain and
// through `?.`.
class Only {
  static only: number = 3;
}

// Nothing here runs — the file fails to compile by design, and the `expect-error`
// headers are its assertions. The collisions that resolve cleanly and their
// runtime behaviour live in `static_instance_name_collision.ts`.
function main(): void {
  const c = new C();
  const bad = c.m;
  console.log(bad);

  const p = new Picker();
  const badChain = p?.pick;
  console.log(badChain);

  const o = new Only();
  const badStatic = o.only;
  console.log(badStatic);
  const badStaticChain = o?.only;
  console.log(badStaticChain);
}
