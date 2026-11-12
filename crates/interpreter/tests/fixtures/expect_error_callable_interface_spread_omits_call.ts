// Object spread on a call-signature interface omits the synthetic `@call` slot,
// so the resulting object type is neither callable nor exposes `@call`.
// expect-error: cannot call value of type `{}`
// expect-error-count: 1
interface UserPred {
  (v: unknown): boolean;
}

function main(): void {
  const b: UserPred = Boolean;
  const copy = { ...b };
  copy(1);
}
