enum SE { A = "abc", B = "def" }
enum NE { X = 2, Y = 3 }
enum Other { Value = 4 }
function choose(flag: boolean): NE | Other { return flag ? NE.X : Other.Value; }
function main(): void {
  const s: SE = SE.A;
  const n: NE = NE.X;
  const union: NE | Other = choose(true);
  const widened: number = union;
  assert(widened === 2 && union + 1 === 3);
  assert(union === 2 && 2 === union);
  assert(union.toFixed(1) === "2.0");
  const mixed: NE | number = n;
  assert(mixed + 1 === 3);
  const text: string = s;
  const number: number = n;
  assert(text === "abc" && number === 2);
  assert("x" + s === "xabc");
  assert(s + SE.B === "abcdef");
  assert(`${s}:${n}` === "abc:2");
  assert(n + 1 === 3);
  assert(n * NE.Y === 6);
  assert(-n === -2);
  assert(n < NE.Y);
  assert(n === 2 && 2 === n);
  assert(s === "abc" && "abc" === s);
  let total = 1;
  total += n;
  assert(total === 3);
  let joined = "x";
  joined += s;
  assert(joined === "xabc");
  assert(s.length === 3);
  assert(s.toUpperCase() === "ABC");
  assert(n.toFixed(1) === "2.0");
  assert((s as string) === "abc");
}
