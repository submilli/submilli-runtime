// `+=` and `++` read and write one slot, and narrowing does not reach that
// target — so a guard around the statement does not help and the message must
// not suggest one. Class, interface, and structural receivers reach the operator
// through three different lookups and have to give the same answer.
//
// The rewrite names the operator the source actually used, the receiver the
// source actually named, and — for `++`/`--`, which mean exactly ±1 — the whole
// statement. A compound assignment's right-hand side stays elided, since it is
// whatever the caller wrote.
//
// expect-error: `f` on `CBox` is nullable; `+=` requires a non-null field
// expect-error: write the assignment out: `if (c.f !== null) { c.f = c.f + …; }`
// expect-error: `f` on `IBox` is nullable; `+=` requires a non-null field
// expect-error: write the assignment out: `if (i.f !== null) { i.f = i.f + …; }`
// expect-error: `f` on `{ f: number | null }` is nullable; `+=` requires a non-null field
// expect-error: `f` on `Opt` is optional; `++` requires a non-null field
// expect-error: write the assignment out: `if (o.f !== null) { o.f = o.f + 1; }`
// expect-error: `n` on `CBox` is nullable; `--` requires a non-null field
// expect-error: write the assignment out: `if (c.n !== null) { c.n = c.n - 1; }`
// expect-error: `b` on `CBox` is nullable; `*=` requires a non-null field
// expect-error: write the assignment out: `if (c.b !== null) { c.b = c.b * …; }`
//
// A field whose non-null form has no `+=` at all is NOT a nullability problem —
// a guard would change nothing — so those fall through to the operator's own
// message rather than blaming the null.
// expect-error: `+=` not defined for `boolean | null` and `boolean`
// expect-error: `+=` not defined for `"a" | "b" | null` and `"b"`
// A numeric literal's arithmetic result widens to number, which cannot be stored
// back into a literal-only field — `lit = lit + 1` would not compile.
// expect-error: postfix `++` expects `number` or `bigint`, found `1 | null`
class CBox {
  f: number | null = 1;
  n: number | null = 1;
  b: bigint | null = 1n;
  flag: boolean | null = true;
  lit: "a" | "b" | null = "a";
  one: 1 | null = 1;
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
    c.f += 1;
  }

  const i: IBox = { f: 1 };
  if (i.f !== null) {
    i.f += 1;
  }

  const s = { f: 1 as number | null };
  if (s.f !== null) {
    s.f += 1;
  }

  const o: Opt = { f: 1 };
  o.f++;

  c.n--;
  c.b *= 2n;

  c.flag += true;
  c.lit += "b";
  c.one++;

  return "x";
}
