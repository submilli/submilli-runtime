// A record read is checked against its value type (SUB-2284): a `never` value
// type admits no value, though a missing key still reads `undefined`, and an
// intersection admits only a value of every member.
interface A { a: number; f(): number }
interface B { b: string }
interface AB { a: number; b: string; f(): number }
function main(): void {
  const numbers: Record<string, number> = { a1: 1 };
  const flags: Record<string, boolean> = { b1: true };
  const merged = Object.assign({}, numbers, flags);
  let threw = false;
  try {
    const y: boolean = merged["a1"];
    assert(typeof y === "boolean");
  } catch (e) {
    threw = e instanceof TypeError;
  }
  assert(threw);
  const key: string = "b1";
  threw = false;
  try {
    const y: boolean = merged[key];
    assert(typeof y === "boolean");
  } catch (e) {
    threw = e instanceof TypeError;
  }
  assert(threw);
  const empty: Record<string, never> = {};
  const missing: string = "zz";
  assert(empty[missing] === undefined && Object.keys(empty).length === 0);
  const { zz } = empty;
  assert(zz === undefined);
  threw = false;
  try {
    const { a1 } = merged;
    assert(typeof a1 === "boolean");
  } catch (e) {
    threw = e instanceof TypeError;
  }
  assert(threw);

  const both: AB = { a: 1, b: "x", f(): number { return 1; } };
  const onlyA: A = { a: 2, f(): number { return 2; } };
  const as: Record<string, A> = { both, onlyA };
  const bs: Record<string, B> = { both };
  const views = Object.assign({}, as, bs);
  const kept = views["both"];
  assert(kept === views["both"]);
  threw = false;
  try {
    views["onlyA"];
  } catch (e) {
    threw = e instanceof TypeError;
  }
  assert(threw);
}
