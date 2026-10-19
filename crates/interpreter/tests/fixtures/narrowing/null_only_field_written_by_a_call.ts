// A field narrowed to `null` and then proven not `null` may have been written
// by a call in between. It reads its declared type less `null`, not `never`,
// so the live value goes through code typed for what it can hold.
function describe(o: { f: null | string }, mutate: () => void): string {
  if (o.f === null) {
    mutate();
    if (o.f !== null) {
      return "now " + String(o.f);
    }
    return "still null";
  }
  return "string";
}

interface Counter { f: 0 | 1 | 2 }

type Even = { kind: "e"; a: number };
type Odd = { kind: "f"; b: string };
type Holder = { u: Even | Odd };

function staleDiscriminant(o: Holder, w: Holder): string {
  if (o.u.kind === "f") {
    w.u = { kind: "e", a: 1 };
    return "b=" + o.u.b;
  }
  return "no";
}

function main(): void {
  const o: { f: null | string } = { f: null };
  assert(describe(o, () => { o.f = "s"; }) === "now s", "the write is seen");
  const p: { f: null | string } = { f: null };
  assert(describe(p, () => {}) === "still null", "no write keeps null");

  const c: Counter = { f: 0 };
  c.f = 1;
  const set = (): void => { c.f = 2; };
  set();
  const one = c.f;
  const seen = one === 1 ? "one" : "other " + String(one);
  assert(seen === "other 2", "a closure's write is seen after the call");

  const h: Holder = { u: { kind: "f", b: "x" } };
  assert(staleDiscriminant(h, h) === "b=undefined", "a missing field reads undefined");
}
