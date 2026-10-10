// A `number | null` / `boolean | null` receiver lowers to the union's ref slot,
// so the value reaching the non-null branch is a *boxed* primitive. The
// per-part access (a Direct prelude wrapper) takes the raw f64/i32, so the
// strip-null step has to unbox, not ref-cast.
function firstFixed(a: number[] | null): string | undefined {
  return a?.at(0)?.toFixed(2);
}

function main(): void {
  const n: number | null = 3.14159;
  assert(n?.toFixed(2) === "3.14", "number receiver unboxes");

  const noNumber: number | null = null as number | null;
  assert(noNumber?.toFixed(2) === undefined, "null number short-circuits");

  const b: boolean | null = true;
  assert(b?.toString() === "true", "boolean receiver unboxes");

  const noBool: boolean | null = null as boolean | null;
  assert(noBool?.toString() === undefined, "null boolean short-circuits");

  // element access hands the next step a boxed element out of the array slot
  assert(firstFixed([3.14159]) === "3.14", "array element continues the chain");
  assert(firstFixed([]) === undefined, "out-of-range element yields undefined");
  assert(firstFixed(null) === undefined, "null array short-circuits");
}
