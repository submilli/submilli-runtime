// expect-error: cannot read field `length`
let shared: string | number = "a";
class Mutator {
  toString(): string { shared = 5; return "changed"; }
}
function main(): number {
  const value = new Mutator();
  if (typeof shared === "string") {
    const text = `${value}`;
    return shared.length + text.length;
  }
  return 0;
}
