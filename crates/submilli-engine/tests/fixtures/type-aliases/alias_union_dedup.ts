// A union is unique by *peeled* type, not by spelling. An alias next to its own
// body is one member, not two — otherwise no operator is defined on the result —
// and an alias whose body is a union contributes that union's members, since a
// nested union hides its `null` from every per-member check downstream,
// including the one that decides whether a slot can hold null at all.
type N = number;
type MaybeNumber = number | null;
type Nums = number[];
type Point = { x: number };
type Spot = { x: number };

class Node {
  v: number = 1;
}
type NodeAlias = Node;

// The flattened union in every slot the nullability probe has to see through:
// a class field, an array element, a parameter, and a return type.
class Holder {
  slot: MaybeNumber | number = null;
}

function widen(v: MaybeNumber | number): MaybeNumber | number {
  return v;
}

function main(): void {
  const a: N | number = 1;
  assert(a + 1 === 2, "arithmetic on an alias-deduped union");
  assert(a.toFixed(2) === "1.00", "method dispatch on an alias-deduped union");

  const b: number | N = 2;
  assert(b * 3 === 6, "same union, other spelling");

  const nothing: MaybeNumber = null;
  const held: MaybeNumber | number = nothing;
  assert(held === null, "a flattened alias union still holds null");
  assert(widen(held) === null, "and survives a round trip through a slot");
  assert(widen(4) === 4, "non-null member of the flattened union");

  const holder = new Holder();
  assert(holder.slot === null, "flattened union in a class field");
  holder.slot = 7;
  assert(holder.slot === 7, "and takes a non-null write");

  const elems: Array<MaybeNumber | number> = [1, null];
  assert(elems[0] === 1, "flattened union holds a number element");
  assert(elems[1] === null, "flattened union holds a null element");

  // Dedup reaches past primitives: array, object, and class aliases all key on
  // the peeled body, so each of these is a one-member union.
  const list: Nums | number[] = [1, 2];
  list.push(3);
  assert(list.length === 3, "array alias deduped against its body");

  const p: Point | Spot = { x: 4 };
  assert(p.x === 4, "two object aliases sharing a body collapse");

  const n: NodeAlias | Node = new Node();
  assert(n instanceof Node, "class alias deduped against its body");
  assert(n.v === 1, "and still reads its fields");
}
