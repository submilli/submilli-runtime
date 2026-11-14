// An object or a class instance has no call or construct signature, so it
// can't stand for an interface that declares one, as in TypeScript (TS2322,
// TS2420).
// expect-error: expected `Tagged`, got `{ label: string }`
// expect-error: expected `Ctor`, got `{}`
// expect-error: expected `Tagged`, got `Plain`
// expect-error: class `Impl` does not implement `Fn`: its instances have no signature
// expect-error: class `CtorImpl` does not implement `Ctor`: its instances have no signature
// expect-error: expected `Derived`, got `{ label: string }`
// expect-error: class `DerivedImpl` does not implement `Derived`: its instances have no signature
// expect-error-count: 7
interface Tagged {
  (n: number): number;
  label: string;
}
interface Fn {
  (n: number): number;
}
interface Ctor {
  new (n: number): Map<string, number>;
}
interface Derived extends Fn {
  label: string;
}
class Plain {
  label: string = "p";
}
class Impl implements Fn {}
class CtorImpl implements Ctor {}
class DerivedImpl implements Derived {
  label: string = "d";
}
function main(): void {
  const literal: Tagged = { label: "x" };
  const empty: Ctor = {};
  const instance: Tagged = new Plain();
  const inherited: Derived = { label: "x" };
}
