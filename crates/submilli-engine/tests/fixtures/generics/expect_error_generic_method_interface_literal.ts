// An object literal for an interface with methods binds the interface's type
// parameters from its members, so a member that disagrees with the others
// is rejected, as in tsc.
// expect-error: field `v`: expected `string`, got `number`
// expect-error-count: 1
interface Getter<T> {
  v: T;
  get(): T;
}

function get<T>(getter: Getter<T>): T {
  return getter.get();
}

function main(): void {
  get({ v: 1, get() { return "s"; } });
}
