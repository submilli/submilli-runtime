// Arithmetic written back to a variable of literal type checks against the
// literal's base type, as in TypeScript: `m += 5` on `m: 1 | 2` writes a
// number. Closures, other functions and loop heads read the variable too, so
// it holds the base type everywhere.
type One = 1 | 2;
let moduleCount: 1 | 2 = 1;
let moduleText: "" = "";

function appendModuleText(): void {
  moduleText += "y";
}

function bump(n: 0 | 1): number {
  n++;
  return n;
}

function main(): void {
  let s: "a" = "a";
  s += "b";
  assert(s === "ab");

  let m: 1 | 2 = 1;
  m += 5;
  m *= 2;
  assert(m === 12);

  let z: 1 = 1;
  z = z + 1;
  assert(z === 2);
  z = (z + 1) * 2;
  assert(z === 6);

  let w: 1 = 1;
  w++;
  assert(w === 2);

  let u: 1 | "a" = 1;
  u = u + 1;
  const n: number = u;
  assert(n === 2);

  let aliased: One = 1;
  aliased++;
  assert(aliased === 2);

  moduleCount += 2;
  moduleCount++;
  assert(moduleCount === 4);

  let captured: "" = "";
  const append = (): void => {
    captured += "x";
  };
  append();
  assert(captured ? true : false);

  let counter: 0 = 0;
  const increment = (): void => {
    counter += 1;
  };
  increment();
  const counted: number = counter || 9;
  assert(counted === 1);

  let looped: 0 = 0;
  let seen = "";
  for (let i = 0; i < 3; i++) {
    seen += looped ? "t" : "f";
    seen += String(looped || 9);
    looped += 1;
  }
  assert(seen === "f9t1t2");

  appendModuleText();
  const text: string = moduleText;
  assert(text === "y");
  assert(bump(1) === 2);
}
