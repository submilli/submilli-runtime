// tsc constant-folds enum initializers, so `-0` becomes `0`: the member is +0.
enum Z { N = -0, Next }

function main(): void {
  assert(1 / Z.N === Infinity, "the member is +0");
  assert(!Object.is(Z.N, -0), "the member is not -0");
  assert(Z.Next === 1, "the next implicit member follows 0");
  console.log(1 / Z.N, Object.is(Z.N, -0), Z.Next);
}
