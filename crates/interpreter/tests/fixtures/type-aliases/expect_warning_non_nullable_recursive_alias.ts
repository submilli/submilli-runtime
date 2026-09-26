// expect-warning: `??` on non-nullable type `NN2` — right side is unreachable
// The nullability probe resolves recursion back-edges, so it must also answer
// *no* correctly: a mutually-recursive pair that never lists `null` is still
// non-nullable, and `??` on it is still the redundancy it was before.

type NN1 = number | NN2[];
type NN2 = NN1 | string;

function main(): void {
  const w: NN2 = 1 as NN2;
  assert((w ?? 5) === 1, "the left side is always taken");
}
