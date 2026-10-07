// A copy `{ ...x }` of a union or a conditional is a copy of whichever
// alternative `x` holds, so it fits any type every alternative fits, as in
// TypeScript: a union of object types, a discriminated union, or a type with
// a dictionary member.
type Shape = { kind: "circle"; r: number } | { kind: "square"; side: number };

interface Dict {
  [k: string]: number;
}

function copy(s: Shape): Shape {
  return { ...s };
}

function area(s: Shape): number {
  switch (s.kind) {
    case "circle":
      return s.r * s.r * 3;
    case "square":
      return s.side * s.side;
  }
}

function keep(x: Dict | { a: string }): Dict | { a: string } {
  return { ...x };
}

function optional(u: Dict | { a: string }): { a?: string } {
  return { ...u };
}

function pick(flag: boolean, circle: Shape, square: Shape): Shape {
  return { ...(flag ? circle : square) };
}

function main(): void {
  const circle = copy({ kind: "circle", r: 2 });
  assert(area(circle) === 12, "a discriminated union copy narrows");
  assert(JSON.stringify(circle) === '{"kind":"circle","r":2}', "the copy holds the fields");

  const square: Shape = { kind: "square", side: 3 };
  const copied: Shape = { ...square };
  if (copied.kind === "square") {
    copied.side = 10;
  }
  assert(square.kind === "square" && square.side === 3, "writing the copy leaves the source");

  const named = keep({ a: "s" });
  assert("a" in named && String(named.a) === "s", "the object alternative");
  assert(JSON.stringify(keep({ z: 5 })) === '{"z":5}', "the dictionary alternative");
  assert(JSON.stringify(optional({ a: "t" })) === '{"a":"t"}', "a target each alternative fits");
  assert(area(pick(false, circle, square)) === 9, "a conditional source");
}
