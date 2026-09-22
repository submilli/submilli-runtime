// expect-error: expected
class Box<T> {
  target(value: T | number | null): void {}
  source(value: T | boolean): void { this.target(value); }
}
export function main(): void {}
