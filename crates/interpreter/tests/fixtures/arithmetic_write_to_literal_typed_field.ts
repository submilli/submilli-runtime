// `++`, `--` and compound arithmetic on a field or array element of literal
// type are checked against the base type of its literal types, as in tsc. The
// field keeps its declared type, and code reading it still sees the value it
// now holds (compared here through a wider binding, as tsc requires).
class Counter {
  n: 5 = 5;
  s: "q" = "q";
}

type Box = { c: 0; s: ""; b: 1 | 2 };

function label(box: Box): string {
  const truthy = box.c ? "t" : "f";
  const filled = box.s ? "u" : "e";
  let which = "none";
  if (box.b === 1) {
    which = "one";
  } else if (box.b === 2) {
    which = "two";
  } else {
    which = "other";
  }
  return truthy + filled + which;
}

function main(): void {
  const box: Box = { c: 0, s: "", b: 2 };
  box.c++;
  box.s += "z";
  box.b++;
  assert(label(box) === "tuother", "fields left their literal types");

  const zeros: 0[] = [0];
  zeros[0]++;
  zeros[0] += 2;
  const element: number = zeros[0];
  assert(element === 3, "an element left its literal type");

  const counter = new Counter();
  counter.n--;
  counter.s += "!";
  const n: number = counter.n;
  const s: string = counter.s;
  assert(n === 4 && s === "q!", "class fields");
}
