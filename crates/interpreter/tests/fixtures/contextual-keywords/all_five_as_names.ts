function of(items: string[]): number {
  return items.length;
}

function type(n: number): void {
  assert(n === 3);
}

function describe(from: string, as: number): string {
  return `${from}:${as}`;
}

function double(type: number): number {
  type = type * 2;
  return type;
}

function main(): void {
  const from = "sender";
  type(3);
  assert(double(4) === 8);

  const is = { is: true, type: "box" };
  assert(is.is);
  assert(is.type === "box");

  const obj = { from: "a", as: 2, of: 3 };
  const { from: renamed, as } = obj;
  assert(renamed === "a");
  assert(as === 2);
  assert(obj.of === 3);

  const shorthand = { from, as };
  assert(shorthand.from === "sender");
  assert(shorthand.as === 2);

  assert(of(["x", "y"]) === 2);
  assert(describe(from, as) === "sender:2");
}
