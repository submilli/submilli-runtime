// A generic alias whose function type has its own type parameter keeps it
// apart from a same-named type parameter it is instantiated with: `Wrap<T>`
// inside `mk<T>` is `<T_1>(x: T_1, u: T) => T_1`.
type Wrap<U> = <T>(x: T, u: U) => T;

function mk<T>(sink: (t: T) => void): Wrap<T> {
  const g: Wrap<T> = <V,>(x: V, u: T): V => {
    sink(u);
    return x;
  };
  return g;
}

function main(): void {
  let seen: string = "";
  const w = mk<string>((s: string): void => {
    seen = s.toUpperCase();
  });
  assert(w<number>(1, "abc") === 1, "the function's own type argument");
  assert(seen === "ABC", "the alias's, kept apart");
}
