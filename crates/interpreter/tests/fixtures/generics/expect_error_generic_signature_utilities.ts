// `Parameters` and `ReturnType` of a generic function type read its own type
// parameters as their bounds, or `unknown`, as tsc does: they don't leak as
// free names that an enclosing type parameter of the same name would match.
// expect-error: expected `number`, got `unknown`
// expect-error-count: 2
type G = <T>(x: T) => T;

function viaReturn<T>(t: T): number {
  const r: ReturnType<G> = t;
  const q: number = r;
  return q;
}

function viaParams<T>(t: T): number {
  const p: Parameters<G> = [t];
  const q: number = p[0];
  return q;
}

function main(): void {}
