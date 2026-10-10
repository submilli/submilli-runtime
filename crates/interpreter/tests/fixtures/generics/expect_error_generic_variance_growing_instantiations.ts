// Instantiations of one interface relate by its measured variances, as in
// tsc, rather than by comparing members whose type arguments grow at every
// level (`I0<I1<T, U>, T>`), which would never finish.
// expect-error: expected `I1<string, 1>`, got `I1<string, number>`
// expect-error-count: 1
interface I0<T, U> {
  m0: I0<I1<T, U>, T>;
  m1: (t: I2<U, I1<U, (x: T) => void>>) => void;
}

interface I1<T, U> {
  m0: I1<U, T>;
  m1: () => I2<U, (x: U) => void>;
  m2: I0<U, U>;
}

interface I2<T, U> {
  m0: I0<() => T, () => (x: T) => void>;
  m1: () => (x: U) => void;
}

function narrower(v: I1<string, number>): I1<string, 1> {
  return v;
}

function main(): void {}
