// An interface whose generic method returns `string` is not assignable to
// one whose method returns `number`, as tsc reports (TS2345).
// expect-error: expected `Expected`, got `Actual`
// expect-error-count: 1
interface Expected {
  value<U>(arg: U): number;
}
interface Actual {
  value<U>(arg: U): string;
}
function take(value: Expected): void {}
function forward(value: Actual): void {
  take(value);
}
function main(): void {}
