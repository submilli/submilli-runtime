// Redeclaring a single accessor half replaces the whole prototype descriptor.
class Base {
  held: number = 10;

  get x(): number {
    return this.held;
  }

  set x(v: number) {
    this.held = v;
  }

  readX(): number {
    return this.x;
  }

  writeX(v: number): void {
    this.x = v;
  }
}

class SetterOnly extends Base {
  last: number = 0;

  set x(v: number) {
    this.last = v;
  }
}

class GetterOnly extends Base {
  get x(): number {
    return 99;
  }
}

function catchTypeError(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

export function main(): string {
  const so = new SetterOnly();
  const soBase: Base = so;
  soBase.writeX(123);

  const go = new GetterOnly();
  const goBase: Base = go;

  return [
    String(so.last),
    String(so.held),
    String((soBase.x as unknown) === undefined),
    String((soBase.readX() as unknown) === undefined),
    String(((so as unknown as Record<string, unknown>)["x"]) === undefined),
    String("x" in so),
    String(Object.hasOwn(so, "x")),
    JSON.stringify(so),
    String(go.x),
    String(goBase.x),
    String(goBase.readX()),
    catchTypeError(() => {
      goBase.x = 5;
    }),
    catchTypeError(() => {
      goBase.writeX(5);
    }),
    catchTypeError(() => {
      (go as unknown as Record<string, unknown>)["x"] = 5;
    }),
    String(go.held),
    String("x" in go),
    String(Object.hasOwn(go, "x")),
    JSON.stringify(go),
  ].join("|");
}
