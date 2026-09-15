// Every rewrite `expect_error_nullable_rw_target.ts` names, pasted back and run.
// A `help:` line that doesn't compile is worse than no help, so the two fixtures
// are a pair: one pins the text, this one proves the text works.

class CBox {
  f: number | null = 1;
  n: number | null = 1;
  b: bigint | null = 1n;
}
interface IBox {
  f: number | null;
}
interface Opt {
  f?: number;
}

export function main(): string {
  const c = new CBox();
  if (c.f !== null) {
    c.f = c.f + 1;
  }
  // `--` names the whole statement, sign included.
  if (c.n !== null) {
    c.n = c.n - 1;
  }
  // The elided right-hand side, filled in with what the caller wrote.
  if (c.b !== null) {
    c.b = c.b * 2n;
  }

  const i: IBox = { f: 1 };
  if (i.f !== null) {
    i.f = i.f + 1;
  }

  const s = { f: 1 as number | null };
  if (s.f !== null) {
    s.f = s.f + 1;
  }

  const o: Opt = { f: 1 };
  if (o.f !== null) {
    o.f = o.f + 1;
  }

  const cf = c.f;
  const cn = c.n;
  const cb = c.b;
  const iv = i.f;
  const sv = s.f;
  const ov = o.f;
  assert(cf !== null && cf === 2, "class `+=` write-out runs");
  assert(cn !== null && cn === 0, "class `--` write-out runs, decrementing");
  assert(cb !== null && cb === 2n, "class `*=` write-out runs on a bigint");
  assert(iv !== null && iv === 2, "interface write-out runs");
  assert(sv !== null && sv === 2, "structural write-out runs");
  assert(ov !== null && ov === 2, "optional write-out runs");
  return "ok";
}
