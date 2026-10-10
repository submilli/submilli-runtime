// A redeclaration collides only with what shares its slot, and statics share
// none: they live in their own namespace, keyed separately from the instance
// members. So a static may take a name an ancestor already uses for an instance
// field, method, or accessor — in either direction — and neither the cross-kind
// rejection nor the compatibility checks apply.

class InstanceBase {
  sv: string = "p";
  sm(): string {
    return "pm";
  }
  get sg(): string {
    return "pg";
  }
}

class StaticChild extends InstanceBase {
  static sv: number = 1;
  static sm(): string {
    return "cm";
  }
}

class StaticBase {
  static sx: string = "sp";
  static sy(): string {
    return "spm";
  }
}

class InstanceChild extends StaticBase {
  sx: number = 2;
  sy(): number {
    return 3;
  }
}

// The cases that actually need the static filter: a static whose *kind* differs
// from the inherited instance member's. Without it these read as cross-kind
// redeclarations, because the ancestor's statics live in their own maps and the
// walk only ever sees the instance member.
class AccessorParent {
  get r(): string {
    return "p";
  }
  get s(): string {
    return "p";
  }
}

class StaticOverAccessor extends AccessorParent {
  static r: number = 1;
  static s(): number {
    return 2;
  }
}

class MethodParent {
  m(): string {
    return "p";
  }
}

class StaticFieldOverMethod extends MethodParent {
  static m: number = 3;
}

class FieldParent {
  f: string = "p";
}

class StaticMethodOverField extends FieldParent {
  static f(): number {
    return 4;
  }
}

function main(): void {
  assert(StaticChild.sv === 1, "a static field does not collide with an inherited instance field");
  assert(
    StaticChild.sm() === "cm",
    "a static method does not collide with an inherited instance method",
  );
  const sc = new StaticChild();
  assert(sc.sv === "p", "the instance member is still the parent's");
  assert(sc.sm() === "pm", "including the inherited method");
  assert(sc.sg === "pg", "and the inherited getter");

  const ic = new InstanceChild();
  assert(ic.sx === 2, "an instance field does not collide with an inherited static");
  assert(ic.sy() === 3, "nor an instance method");
  assert(StaticBase.sx === "sp", "and the static is untouched");
  assert(InstanceChild.sy() === "spm", "the inherited static is still reachable on the child");

  assert(StaticOverAccessor.r === 1, "a static field over an inherited accessor");
  assert(StaticOverAccessor.s() === 2, "a static method over an inherited accessor");
  assert(new StaticOverAccessor().r === "p", "the inherited accessor still answers the instance");
  assert(StaticFieldOverMethod.m === 3, "a static field over an inherited method");
  assert(new StaticFieldOverMethod().m() === "p", "the inherited method still answers the instance");
  assert(StaticMethodOverField.f() === 4, "a static method over an inherited field");
  assert(new StaticMethodOverField().f === "p", "the inherited field still answers the instance");
}
