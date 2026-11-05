// expect-error: object literal is missing required field `a` of type `Row`
// expect-error: interface Row {
// expect-error: no variant `Green` on enum `Color`
// expect-error: type alias `Pair` expects 1 type argument, got 0
// expect-error: interface `Box` expects 1 type argument, got 0
// expect-error: interface Box<U>
// expect-error: two different types here are named `Row`, declared in different scopes
// expect-error: did you mean `Point`?
// expect-error-count: 7
// Diagnostics name a local type by its source name, and count only the type
// parameters the source writes.

function main(): void {
  interface Row { a: number; }
  const r: Row = { b: 1 };
  enum Color { Red, Blue }
  const c = Color.Green;
}

interface Row { a: string; }

function shadowed(top: Row): void {
  interface Row { a: number; }
  const r: Row = top;
}

function typo(): void {
  interface Point { x: number; }
  const p: Pont = { x: 1 };
}

function pairs<T>(x: T): void {
  type Pair<U> = [T, U];
  const p: Pair = [x, x];
  interface Box<U> { t: T; u: U; }
  const b: Box = { t: x, u: x };
}
