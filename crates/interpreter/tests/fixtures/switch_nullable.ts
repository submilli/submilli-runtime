function classify(x: string | number | boolean | null): number {
  switch (x) {
    case "a": return 1;
    case 2: return 2;
    case true: return 3;
    case null: return 4;
    default: return 5;
  }
}
enum Num { A = 1, B = 2 }
enum Str { A = "a", B = "b" }
function numEnum(x: Num | null): number {
  switch (x) { case Num.A: return 1; case Num.B: return 2; default: return 0; }
}
function strEnum(x: Str | null): number {
  switch (x) { case Str.A: return 1; case Str.B: return 2; default: return 0; }
}
let evaluated: number = 0;
function discriminant(): number | null { evaluated += 1; return 2; }
function main(): void {
  assert(numEnum(Num.A) === 1);
  assert(numEnum(Num.B) === 2);
  assert(numEnum(null) === 0);
  assert(strEnum(Str.A) === 1);
  assert(strEnum(Str.B) === 2);
  assert(strEnum(null) === 0);
  switch (discriminant()) { case 1: assert(false); break; case 2: break; default: assert(false); }
  assert(evaluated === 1, "switch discriminant evaluated once");

  assert(classify("a") === 1);
  assert(classify(2) === 2);
  assert(classify(true) === 3);
  assert(classify(null) === 4);
  assert(classify("b") === 5);
  assert(classify(false) === 5);
  assert(classify(3) === 5);
}
