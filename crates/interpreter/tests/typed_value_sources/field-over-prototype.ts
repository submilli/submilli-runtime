// Field over a prototype method or accessor (useDefineForClassFields semantics).
class BaseMethod {
  baseField: number = 10;
  beforeInit: number = 0;

  constructor() {
    this.beforeInit = this.m();
  }

  m(): number {
    return 1;
  }

  call(): number {
    return this.m() * 10;
  }
}

class SubMethod extends BaseMethod {
  pre: number = this.m();
  m: () => number = (): number => 2;
  post: number = this.m();

  viaSuper(): number {
    return super.m();
  }
}

class BaseAccessor {
  log: string[] = [];

  constructor() {
    this.log.push("ctor:" + this.x);
  }

  get x(): number {
    this.log.push("get");
    return 10;
  }

  set x(v: number) {
    this.log.push("set:" + v);
  }

  readX(): number {
    return this.x;
  }

  writeX(v: number): void {
    this.x = v;
  }
}

class SubAccessor extends BaseAccessor {
  before: number = this.x;
  x: number = 20;
  after: number = this.x;
  tail: number = 30;
}

class SubUninitAccessor extends BaseAccessor {
  x?: number;
}

function keysViaForIn(o: object): string {
  const out: string[] = [];
  for (const k in o) {
    out.push(k);
  }
  return out.join(",");
}

export function main(): string {
  const b = new BaseMethod();
  const d = new SubMethod();
  const asBase: BaseMethod = d;
  const bound = asBase.m;

  const spreadD = { ...d };
  const methodResults = [
    String(d.beforeInit),
    String(d.pre),
    String(d.post),
    String(d.m()),
    String(asBase.m()),
    String(d.call()),
    String(bound()),
    String(d.viaSuper()),
    String(b.m()),
    String(b.call()),
    Object.keys(d).join(","),
    keysViaForIn(d),
    Object.keys(spreadD).join(","),
    String(Object.hasOwn(d, "m")),
    String("m" in d),
    String(Object.hasOwn(b, "m")),
    String("m" in b),
    JSON.stringify(d),
  ];

  delete (d as { m?: () => number }).m;
  const afterDelete = [
    String(d.call()),
    String(asBase.m()),
    String(Object.hasOwn(d, "m")),
    String("m" in d),
  ];

  const s = new SubAccessor();
  const asAccBase: BaseAccessor = s;
  const preWriteLog = s.log.join(",");
  s.writeX(25);
  const spreadS = { ...s };
  const accResults = [
    String(s.before),
    String(s.after),
    String(s.x),
    String(asAccBase.x),
    String(s.readX()),
    preWriteLog,
    s.log.join(","),
    Object.keys(s).join(","),
    keysViaForIn(s),
    String(Object.hasOwn(s, "x")),
    String("x" in s),
    String(spreadS.x),
    JSON.stringify(s),
  ];

  const u = new SubUninitAccessor();
  const uninitResults = [
    String(Object.hasOwn(u, "x")),
    String((u.x as unknown) === undefined),
    String(((u as BaseAccessor).readX() as unknown) === undefined),
    Object.keys(u).join(","),
  ];

  return [
    methodResults.join("|"),
    afterDelete.join("|"),
    accResults.join("|"),
    uninitResults.join("|"),
  ].join(" // ");
}
