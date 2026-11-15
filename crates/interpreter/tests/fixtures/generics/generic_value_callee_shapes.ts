// A generic function value reached through other callee shapes — an instance
// field, a getter, `this`, a call result, a tuple, `Record`, array and `Map`
// elements — takes each call's type arguments.
import session from "submilli:session";
function rejects(run: () => void): boolean {
  try { run(); } catch (e: TypeError) { return e.message.indexOf("does not match") >= 0; }
  return false;
}
type Load = <T>(key: string) => T | undefined;
const load = <T,>(key: string): T | undefined => session.get<T>(key);
class K {
  f: Load = load;
  get g(): Load { return load; }
  viaThis(): number | undefined { return this.f<number>("num"); }
}
function mk(): Load { return load; }
function main(): void {
  session.set("num", 1);
  session.set("str", "x");
  const k = new K();
  assert(k.f<number>("num") === 1, "instance field");
  assert(rejects(() => { k.f<string>("num"); }), "instance field binds");
  assert(k.g<number>("num") === 1, "getter");
  assert(rejects(() => { k.g<string>("num"); }), "getter binds");
  assert(k.viaThis() === 1, "this field");
  assert(mk()<number>("num") === 1, "call result");
  assert(rejects(() => { mk()<string>("num"); }), "call result binds");
  const tup: [Load] = [load];
  assert(tup[0]<number>("num") === 1, "tuple element");
  assert(rejects(() => { tup[0]<string>("num"); }), "tuple element binds");
  const rec: Record<string, Load> = { a: load };
  const ra = rec["a"];
  if (ra !== undefined) {
    assert(ra<number>("num") === 1, "record value");
    assert(rejects(() => { ra<string>("num"); }), "record value binds");
  }
  const arr: Load[] = [load];
  const a0 = arr[0];
  if (a0 !== undefined) { assert(rejects(() => { a0<string>("num"); }), "array element binds"); }
  const m = new Map<string, Load>();
  m.set("a", load);
  const mg = m.get("a");
  if (mg !== undefined) { assert(rejects(() => { mg<string>("num"); }), "map value binds"); }
  const s: string | undefined = k.f("str");
}
