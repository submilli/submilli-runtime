// A `case` naming an enum member matches that member, and a bare literal
// `case` the literal, as in TypeScript: both may sit in one switch, and a
// switch naming every member of each enum, or with `case null`, matches every
// value.
enum E {
  A,
  B,
}
enum F {
  X,
  Y,
}
enum Twin {
  First = 0,
  Second = 0,
}

function bothEnums(v: E | F): string {
  switch (v) {
    case E.A:
      return "ea";
    case E.B:
      return "eb";
    case F.X:
      return "fx";
    case F.Y:
      return "fy";
  }
}

function enumAndLiterals(v: E | 0 | 1): string {
  switch (v) {
    case E.A:
      return "ea";
    case E.B:
      return "eb";
    case 0:
      return "0";
    case 1:
      return "1";
  }
}

function withNull(v: E | null): string {
  switch (v) {
    case null:
      return "null";
    case E.A:
      return "a";
    case E.B:
      return "b";
  }
}

function twins(v: Twin): string {
  switch (v) {
    case Twin.First:
      return "first";
    case Twin.Second:
      return "second";
  }
}

function literalLeft(v: E | 0): string {
  switch (v) {
    case E.A:
      return "a";
    case E.B:
      return "b";
    default: {
      const zero: 0 = v;
      return "zero " + String(zero);
    }
  }
}

enum Tag {
  P = "p",
  Q = "q",
}
interface TaggedByEnum {
  tag: Tag;
  c: number;
}
interface TaggedQ {
  tag: "q";
  d: string;
}

// `case Tag.Q` and `case "q"` leave only `Tag.P`, which no `TaggedQ` holds.
function partlyNamed(u: TaggedByEnum | TaggedQ): number {
  switch (u.tag) {
    case Tag.Q:
      return 1;
    case "q":
      return 2;
    default:
      return u.c;
  }
}

function main(): void {
  assert(bothEnums(F.X) === "ea");
  assert(enumAndLiterals(0) === "ea");
  assert(withNull(null) === "null");
  assert(withNull(E.B) === "b");
  assert(twins(Twin.Second) === "first");
  assert(literalLeft(E.B) === "b");
  assert(partlyNamed({ tag: Tag.P, c: 7 }) === 7);
}
