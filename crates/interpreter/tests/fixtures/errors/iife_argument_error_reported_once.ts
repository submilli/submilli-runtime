// expect-error: unresolved identifier `nope`
// An immediately-invoked function's argument in error reports itself; its
// unannotated parameter takes the error rather than asking for an annotation.
function main(): void {
  const r = ((x) => x)(nope);
}
