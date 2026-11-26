class A {
  m(x: number): number {
    return x;
  }
}

class B extends A {
  m(x: number, y: number = 10): number {
    return x + y;
  }
}

class C extends B {
  m(x: number, y: number = 20, z: number = 30): number {
    return super.m(x, y) + z;
  }
}

class D extends B {}

let defaultRuns = 0;
function bumpDefault(val: number): number {
  defaultRuns = defaultRuns + 1;
  return val;
}

class BThis extends A {
  bias: number = 100;
  m(x: number, y: number = bumpDefault(this.bias + x * 2)): number {
    return x + y;
  }
}

class AStr {
  m(x: number): string {
    return `${x}`;
  }
}

class BOpt extends AStr {
  m(x: number, y?: number): string {
    return `${x}:${y === undefined ? "undef" : y}`;
  }
}

class RBase {
  m(x: number, y: number): number {
    return x + y;
  }
}

class RSub extends RBase {
  m(...xs: number[]): number {
    return xs.reduce((acc, v) => acc + v, 0);
  }
}

class BRestAfterFixed extends A {
  m(x: number, ...ys: number[]): number {
    return x * 100 + ys.length * 10 + ys.reduce((acc, v) => acc + v, 0);
  }
}

class BCombo extends A {
  m(x: number, y: number = 10, ...zs: number[]): number {
    return x + y + zs.length * 100 + zs.reduce((acc, v) => acc + v, 0);
  }
}

class OptBase {
  m(x: number, y?: number): number {
    return x + (y ?? 0);
  }
}

class RestOverOpt extends OptBase {
  m(...xs: (number | undefined)[]): number {
    return xs.length;
  }
}

interface IA {
  m(x: number): number;
}

class BComboImpl extends A implements IA {
  m(x: number, y: number = 10, ...zs: number[]): number {
    return x + y + zs.length * 100 + zs.reduce((acc, v) => acc + v, 0);
  }
}

interface IR {
  m(x: number, y: number): number;
}

class GuardParent {
  v: unknown = null;
  setV(v: unknown): void {
    this.v = v;
  }
}

class GuardChild<T> extends GuardParent {
  v: T;
  constructor(v: T) {
    super();
    this.v = v;
  }
  readV(): T {
    return this.v;
  }
}

function verifyRuntimeT<T>(sample: T, candidate: unknown): T {
  const g = new GuardChild<T>(sample);
  g.setV(candidate);
  return g.readV();
}

class GBase {
  id<T>(sample: T, candidate: unknown): T {
    return verifyRuntimeT<T>(sample, candidate);
  }
}

class GSub extends GBase {
  id<T>(sample: T, candidate: unknown, tag: string = "ok"): T {
    assert(tag.length > 0, "tag present");
    return verifyRuntimeT<T>(sample, candidate);
  }
}

class GOptBase {
  m<T>(x: T, y?: T): number {
    return y === undefined ? 10 : 20;
  }
}

class GRestOverOpt extends GOptBase {
  m<T>(...xs: (T | undefined)[]): number {
    if (xs.length > 0 && xs[0] !== undefined) {
      const first: T = xs[0];
      const checked: T = verifyRuntimeT<T>(first, first);
      assert(checked === first, "generic T preserved in rest override");
    }
    return xs.length;
  }
}

class GDefBase {
  m<T>(x: T, n: number = 10): number {
    return n;
  }
}

class GDefSub extends GDefBase {
  m<T>(x: T, n: number = 50, extra: number = 7): number {
    const checked: T = verifyRuntimeT<T>(x, x);
    assert(checked === x, "generic T preserved in default override");
    return n + extra;
  }
}

class GAdaptBase {
  id<T>(sample: T, candidate: unknown, extra?: number): T {
    return verifyRuntimeT<T>(sample, candidate);
  }
}

class GAdaptSub extends GAdaptBase {
  id<T>(sample: T, candidate: unknown, extra: number = 10, tag: string = "ok"): T {
    assert(tag.length > 0 && extra === 10, "adapted generic method defaults");
    return verifyRuntimeT<T>(sample, candidate);
  }
}

class SuperOmitChild extends B {
  m(x: number, y: number = 20): number {
    return super.m(x) * 100 + y;
  }
}

class SuperRestParent {
  m(x: number, ...ys: number[]): number {
    return x * 100 + ys.length * 10 + ys.reduce((a, b) => a + b, 0);
  }
}

class SuperRestChild extends SuperRestParent {
  m(x: number, ...ys: number[]): number {
    return super.m(x, 2, 3) + ys.length;
  }
}

class Box<T> {
  put(x: T): string {
    return `${String(x)}`;
  }
}

class NumBox extends Box<number> {
  put(x: number, scale: number = 10): string {
    return `${x * scale}`;
  }
}

let voidAcc = 0;
class VoidBase {
  act(x: number): void {
    voidAcc = voidAcc + x;
  }
}

class VoidSub extends VoidBase {
  act(x: number, mult: number = 3): void {
    voidAcc = voidAcc + x * mult;
  }
}

class FreshRestBase {
  m(a: number, b: number): number {
    return a + b;
  }
}

class FreshRestSub extends FreshRestBase {
  m(...xs: number[]): number {
    xs.push(99);
    return xs.length;
  }
}

const events: string[] = [];
function recordDef(): number {
  events.push("def");
  return 10;
}

class OrderBase {
  m(x: number): number {
    return x;
  }
}

class OrderSub extends OrderBase {
  m(x: number, y: number = recordDef()): number {
    events.push("body");
    return x + y;
  }
}

function main(): void {
  const b = new B();
  assert(b instanceof A, "b instanceof A");
  assert(b.m(1, 2) === 3, "b.m(1, 2)");
  assert(b.m(1) === 11, "b.m(1)");
  assert(b.m(1, undefined) === 11, "b.m(1, undefined)");
  assert((b as A).m(1) === 11, "(b as A).m(1)");

  const c = new C();
  assert((c as A).m(1) === 51, "(c as A).m(1)");
  assert((c as B).m(1, 2) === 33, "(c as B).m(1, 2)");
  assert((c as B).m(1) === 51, "(c as B).m(1)");
  assert(c.m(1, 2, 3) === 6, "c.m(1, 2, 3)");

  const d = new D();
  assert((d as A).m(5) === 15, "(d as A).m(5)");
  assert(d.m(5, 6) === 11, "d.m(5, 6)");
  assert(d.m(5) === 15, "d.m(5)");

  const bt = new BThis();
  assert((bt as A).m(3) === 109, "(bt as A).m(3)");
  assert(defaultRuns === 1, "default ran once");
  assert(bt.m(3, 4) === 7, "bt.m(3, 4)");
  assert(defaultRuns === 1, "default skipped when arg supplied");

  const bo = new BOpt();
  assert(bo.m(1, 2) === "1:2", "bo.m(1, 2)");
  assert(bo.m(1) === "1:undef", "bo.m(1)");
  assert((bo as AStr).m(1) === "1:undef", "(bo as AStr).m(1)");

  const rs = new RSub();
  assert(rs.m(1, 2, 3) === 6, "rs.m(1, 2, 3)");
  assert(rs.m() === 0, "rs.m()");
  assert((rs as RBase).m(4, 5) === 9, "(rs as RBase).m(4, 5)");

  const br = new BRestAfterFixed();
  assert(br.m(2, 3, 4) === 227, "br.m(2, 3, 4)");
  assert(br.m(2) === 200, "br.m(2)");
  assert((br as A).m(2) === 200, "(br as A).m(2)");

  const bc = new BCombo();
  assert((bc as A).m(1) === 11, "(bc as A).m(1)");
  assert(bc.m(1, 2, 3, 4) === 210, "bc.m(1, 2, 3, 4)");

  const roo: OptBase = new RestOverOpt();
  assert(roo.m(7) === 1, "roo.m(7) omitted optional");
  assert(roo.m(7, undefined) === 2, "roo.m(7, undefined) explicit undefined");
  assert(roo.m(7, 8) === 2, "roo.m(7, 8)");

  const ia: IA = new B();
  assert(ia.m(5) === 15, "IA on B");
  const iaC: IA = new C();
  assert(iaC.m(1) === 51, "IA on C");
  const iaCombo: IA = new BComboImpl();
  assert(iaCombo.m(1) === 11, "IA on BComboImpl");
  const ir: IR = new RSub();
  assert(ir.m(10, 20) === 30, "IR on RSub");

  const os = new OrderSub();
  const getRecv = (): OrderBase => {
    events.push("recv");
    return os;
  };
  const getArg = (): number => {
    events.push("arg");
    return 5;
  };
  assert(getRecv().m(getArg()) === 15, "eval order result");
  assert(events.join(",") === "recv,arg,def,body", "eval order sequence");

  const gs = new GSub();
  assert((gs as GBase).id<number>(0, 42) === 42, "(gs as GBase).id<number>(0, 42)");
  assert(gs.id<string>("", "hi", "custom") === "hi", "gs.id<string>('', 'hi', 'custom')");
  assert(gs.id<string>("", "hi") === "hi", "gs.id<string>('', 'hi')");
  let threwCast = false;
  try {
    (gs as GBase).id<number>(0, "not-a-number");
  } catch (e) {
    threwCast = e instanceof TypeError;
  }
  assert(threwCast, "generic T descriptor enforced through base slot");

  const gro: GOptBase = new GRestOverOpt();
  assert(gro.m<number>(7) === 1, "GRestOverOpt omitted optional");
  assert(gro.m<number>(7, undefined) === 2, "GRestOverOpt explicit undefined");
  assert(gro.m<number>(7, 8) === 2, "GRestOverOpt both args");

  const gds: GDefBase = new GDefSub();
  assert(gds.m<string>("a") === 57, "GDefSub uses own defaults when omitted");
  assert(gds.m<string>("a", 3) === 10, "GDefSub explicit arg + trailing default");

  const soc = new SuperOmitChild();
  assert((soc as A).m(4) === 1420, "super.m(x) omits B's defaulted param");

  const src = new SuperRestChild();
  assert(src.m(5, 8, 9) === 527, "super.m(x, 2, 3) into rest parent");

  const nb: Box<number> = new NumBox();
  assert(nb.put(6) === "60", "erased generic class override with default");

  const vs: VoidBase = new VoidSub();
  vs.act(4);
  assert(voidAcc === 12, "void-returning override with default");

  const extracted = (b as unknown as { [k: string]: unknown })["m"] as (x: number) => number;
  assert(extracted(5) === 15, "extracted method value preserves default");

  const bOverwrite = new B();
  (bOverwrite as { m: (x: number) => number }).m = (x: number): number => x * 100;
  assert((bOverwrite as A).m(3) === 300, "own property shadow wins over vtable thunk");

  const fr: FreshRestBase = new FreshRestSub();
  assert(fr.m(1, 2) === 3, "first rest call gets fresh array");
  assert(fr.m(1, 2) === 3, "second rest call gets fresh array");

  const gaSub = new GAdaptSub();
  const adapted: <T>(sample: T, candidate: unknown, extra?: number) => T = (
    gaSub as { id: <T>(sample: T, candidate: unknown, extra?: number) => T }
  ).id;
  const gaBase = new GAdaptBase();
  (gaBase as { id: <T>(sample: T, candidate: unknown, extra?: number) => T }).id = adapted;
  assert(gaBase.id<number>(0, 42) === 42, "adapted generic method with omitted optional");
  let threwAdaptedCast = false;
  try {
    gaBase.id<number>(0, "not-a-number");
  } catch (e) {
    threwAdaptedCast = e instanceof TypeError;
  }
  assert(threwAdaptedCast, "adapted generic method enforces T descriptor");

  const hybridAdapted = Object.assign(adapted as unknown as {}, {
    label: "h",
  }) as unknown as <T>(sample: T, candidate: unknown, extra?: number) => T;
  (gaBase as { id: <T>(sample: T, candidate: unknown, extra?: number) => T }).id = hybridAdapted;
  assert(gaBase.id<number>(0, 99) === 99, "hybrid-wrapped adapted generic method with omitted optional");
}
