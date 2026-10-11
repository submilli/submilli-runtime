function reset(value: number | undefined = 4): undefined {
  value = undefined;
  return value;
}
function change(value: number | undefined = 4): number | undefined {
  const clear = (): void => { value = undefined; };
  clear();
  return value;
}
function main(): void {
  assert(reset() === undefined, "declared optional type permits later undefined assignment");
  assert(change() === undefined, "captured write invalidates initialized read refinement");
}
