// expect-error: is a method on `CA`, and a union receiver reads only fields
// expect-error: cannot assign to `x` through `CA | CB`
// expect-error: cannot read field `x` on `null | IA`: the receiver can be `null`
// The three things a nominal-union receiver still can't do. Each diagnostic
// names which of them it is — "does not exist" for a member that visibly
// declares the name would send the reader looking for the wrong fix — and the
// write names the narrowing that makes it legal.
// expect-error: `if (v instanceof CA) { v.x = … }`
class CA {
  x: number = 1;
  m(): number {
    return 1;
  }
}

class CB {
  x: number = 2;
  m(): number {
    return 2;
  }
}

interface IA {
  x: number;
}

function methodOnUnion(v: CA | CB): number {
  return v.m();
}

function writeThroughUnion(v: CA | CB): void {
  v.x = 9;
}

function nullableUnion(v: IA | null): number {
  return v.x;
}

function main(): void {
  assert(methodOnUnion(new CA()) === 1, "unreachable — the program does not compile");
  writeThroughUnion(new CB());
  assert(nullableUnion(null) === 0, "unreachable");
}
