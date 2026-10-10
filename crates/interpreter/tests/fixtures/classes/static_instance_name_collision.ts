// A class may declare a static and an instance member of the same name
// (spec.md §Classes), and an instance receiver resolves to the *instance* one.
// These are the collisions that resolve cleanly and run; the ones that produce a
// diagnostic are in `expect_error_static_instance_name_collision.ts`, which
// cannot assert on runtime values because it never runs.

class Both {
  static v: number = 1;
  v: number = 2;
}

class StaticMethodInstanceField {
  static w(): number {
    return 1;
  }
  w: number = 2;
}

class StaticFieldInstanceMethod {
  static x: number = 1;
  x(): number {
    return 2;
  }
}

class Collide {
  static m(): number {
    return 1;
  }
  m(a: number): number {
    return a + 10;
  }
}

function main(): void {
  const c = new Collide();
  assert(c.m(5) === 15, "a call on an instance resolves to the instance method");
  assert(Collide.m() === 1, "the static of the same name is still callable on the class");

  const b = new Both();
  assert(b.v === 2, "an instance field wins over a same-named static");
  assert(b?.v === 2, "an instance field wins over a same-named static through `?.` too");
  assert(Both.v === 1, "the static field is still reachable on the class");

  const smif = new StaticMethodInstanceField();
  assert(smif.w === 2, "an instance field wins over a same-named static method");
  assert(StaticMethodInstanceField.w() === 1, "the static method is still callable");

  const sfim = new StaticFieldInstanceMethod();
  assert(sfim.x() === 2, "an instance method wins over a same-named static field");
  assert(StaticFieldInstanceMethod.x === 1, "the static field is still readable");
}
