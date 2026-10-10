// A rest parameter may be a `readonly` array: the callee can only read it, and
// since every call packs a fresh array, function types relate on the rest's
// elements whether or not either side is `readonly`.
class Joiner {
  join(separator: string, ...parts: readonly string[]): string {
    return parts.join(separator);
  }
}

function count(...args: readonly string[]): number {
  return args.length;
}

function firstOf<T>(...items: readonly T[]): T | null {
  return items.length > 0 ? items[0] : null;
}

function grow(...items: string[]): number {
  items.push("extra");
  return items.length;
}

function callReadonly(f: (...items: readonly string[]) => number): number {
  return f("a", "b");
}

function main(): void {
  assert(count() === 0 && count("a", "b") === 2, "direct calls");
  assert(firstOf(4, 5) === 4 && firstOf<string>() === null, "generic");
  assert(new Joiner().join("-", "a", "b") === "a-b", "method");

  const sum = (...xs: readonly number[]): number => {
    let total = 0;
    for (const x of xs) {
      total += x;
    }
    return total;
  };
  assert(sum(1, 2, 3) === 6, "arrow");

  const asMutable: (...xs: number[]) => number = sum;
  assert(asMutable(4, 5) === 9, "readonly rest where a mutable one is expected");
  assert(callReadonly(grow) === 3, "mutable rest where a readonly one is expected");
  assert(callReadonly((...ys: string[]) => ys.length) === 2, "contextually typed arrow");
}
