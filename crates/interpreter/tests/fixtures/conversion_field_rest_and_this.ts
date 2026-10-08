// A `toString` or `toJson` field may hold a function with a rest parameter,
// which the conversion calls with no arguments, and a function expression
// there sees the object as `this`.
interface Shown {
  toString?: () => string;
}

function count(...xs: number[]): string {
  return "v" + String(xs.length);
}

function quoted(...xs: string[]): string {
  return '"' + String(xs.length) + '"';
}

function main(): void {
  const inferred = { toString: count };
  assert(String(inferred) === "v0", "an inferred rest toString");
  const declared: Shown = { toString: count };
  assert(String(declared) === "v0", "a declared rest toString");
  assert(`${declared}` === "v0", "interpolating a rest toString");
  assert(JSON.stringify({ toJson: quoted }) === '"0"', "a rest toJson");

  const self = {
    x: 2,
    toString: function (): string {
      return "x=" + String(this.x);
    },
  };
  assert(String(self) === "x=2", "a toString function expression's this");
  assert([self, self].join("|") === "x=2|x=2", "a function expression's this through join");
}
