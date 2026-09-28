// A closure in a top-level block can't yet capture that block's bindings
// (SUB-1070), which a function declared there needs even to call itself, so
// declaring one there is rejected. Functions in top-level arrows are not.
// expect-error: a function can't be declared in a top-level block yet
// expect-error-count: 2
{
  function count(n: number): number {
    return n <= 0 ? 0 : count(n - 1) + 1;
  }
  console.log(count(2));
}
for (let i = 0; i < 2; i++) {
  function show(): number {
    return i;
  }
  console.log(show());
}
const run = (): number => {
  function inner(): number {
    return 1;
  }
  return inner();
};
function main(): void {
  console.log(run());
}
