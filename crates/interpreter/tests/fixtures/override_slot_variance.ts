class Base {
  n: number = 0;
  pick(): string | number { return "base"; }
  flag(): string | boolean { return "base"; }
  set x(v: number) { this.n = v; }
  put(v: string): void { this.n = v.length; }
}
class Child extends Base {
  pick(): number { return 42; }
  flag(): boolean { return true; }
  set x(v: number | null) { this.n = v === null ? -1 : v; }
  put(v: string | null): void { this.n = v === null ? -2 : v.length; }
}
interface Picker { pick(): string | number; flag(): string | boolean; }
class GetterBase { get x(): string | number { return "base"; } }
class GetterChild extends GetterBase { get x(): number { return 7; } }
class VoidBase { run(): void {} }
class NeverChild extends VoidBase { run(): never { throw new Error("boom"); } }
function main(): void {
  let caught = 0;
  const never = new NeverChild();
  const voidBase: VoidBase = never;
  try { never.run(); } catch (e) { caught++; }
  try { voidBase.run(); } catch (e) { caught++; }
  assert(caught === 2, "never override of a void slot");
  const child = new Child();
  assert(child.pick() === 42, "narrowed number return");
  assert(child.flag(), "narrowed boolean return");
  const base: Base = child;
  assert(base.pick() === 42, "base dispatch");
  const shape: Picker = child;
  assert(shape.pick() === 42, "interface dispatch");
  assert(shape.flag() === true, "interface boolean dispatch");
  child.x = null;
  assert(child.n === -1, "widened setter");
  base.x = 9;
  assert(child.n === 9, "base setter");
  child.put(null);
  assert(child.n === -2, "widened method");
  base.put("abc");
  assert(child.n === 3, "base method");
  assert(new GetterChild().x === 7, "narrowed getter");
}
