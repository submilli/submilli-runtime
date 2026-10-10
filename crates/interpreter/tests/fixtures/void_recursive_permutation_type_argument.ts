interface Swap<A, B> { read(): A; write(value: B): void; next: Swap<B, A> }
function inspect(value: Swap<void, number>): void {}
function main(): void {
  const optional: { value?: Swap<void, number> } = {};
  assert(optional.value === undefined, "recursive permutation accepts void parameters");
}
