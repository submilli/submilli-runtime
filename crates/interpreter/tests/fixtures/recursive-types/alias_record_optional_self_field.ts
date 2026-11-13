// A record alias with an optional field of its own type: an object literal or
// an object rest of it is laid out with the field's rehydrated type, the one
// its vtable is recorded under (SUB-2283).
type T = { [k: string]: T | undefined; meta?: T };
type Prefixed = { [k: `a${string}`]: Prefixed | undefined; meta?: Prefixed };
type Two = {
  [k: string]: Two | number | undefined;
  [k: `n${string}`]: number | undefined;
  meta?: Two;
};
type Linked = { next?: Linked; v: number; [k: string]: Linked | number | undefined };
function main(): void {
  const a: T = {};
  assert(a.meta === undefined);
  const nested: T = { meta: {} };
  assert(nested.meta !== undefined && nested.meta.meta === undefined);
  const prefixed: Prefixed = {};
  assert(prefixed.meta === undefined);
  const two: Two = { n1: 2 };
  assert(two.meta === undefined && two["n1"] === 2);
  const linked: Linked = { v: 1, next: { v: 2 } };
  assert(linked.next?.v === 2 && linked.next.next === undefined);
  const copy: T = { ...nested };
  assert(copy.meta !== undefined);
  const { meta, ...others } = nested;
  assert(meta !== undefined && others.meta === undefined);
  const { v, ...tail } = linked;
  assert(v === 1 && tail.next?.v === 2);
}
