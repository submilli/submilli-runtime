// A type parameter list may not name the same parameter twice (TS2300): the
// second would shadow the first, leaving it unreachable. Every list goes through
// one parser, so each declaration form is covered by checking one of each.
// expect-error: duplicate type parameter `T`
// expect-error: duplicate type parameter `U`
// expect-error: duplicate type parameter `V`
// expect-error: duplicate type parameter `W`
function f<T, T>(x: T): T {
  return x;
}
class C<U, U> {
  v: U;
  constructor(v: U) {
    this.v = v;
  }
}
interface I<V, V> {
  a: V;
}
type A<W, W> = W;

function main(): void {}
