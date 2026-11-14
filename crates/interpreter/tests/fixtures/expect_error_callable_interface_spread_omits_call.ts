// Object spread on a call-signature interface copies no call signature, so the
// resulting object type is not callable, as in TypeScript.
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
