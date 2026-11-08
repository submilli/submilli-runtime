// A bound that names its own parameter is substituted once, not expanded.
function child<T extends { left: T | null; right: T | null }>(node: T, side: "left" | "right"): T | null { return node[side]; }
function first<T extends { a: T[keyof T] }>(value: T): T[keyof T] { return value.a; }
function grandchild<T extends { a: T }>(value: T, inner: T["a"]["a"]): T["a"]["a"] { return inner; }
function nextValue<T extends { next: T | null; v: number }>(node: T, value: NonNullable<T["next"]>["v"]): number { const read: number = value; return read + node.v; }
interface Tree { left: Tree | null; right: Tree | null }
interface Loop { a: Loop | null }
interface Linked { next: Linked | null; v: number }

function main(): void {
  const leaf: Tree = { left: null, right: null };
  const root: Tree = { left: leaf, right: null };
  assert(child(root, "left") === leaf && child(root, "right") === null, "an F-bounded receiver reads its children");
  assert(first({ a: 1 }) === 1, "a bound reading its own keys");
  const list: Linked = { next: null, v: 3 };
  assert(nextValue(list, 4) === 7, "a utility over a self-referencing bound reads its keys");
}
