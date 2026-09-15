// The user-visible contract: a callback narrower than the parameter it fills is
// refused, so the subtype-widening that lets `Map<Base,V>#set(new Sub())`
// through never reaches a contravariant position.
//
// Note this is pinned at the *behaviour* level, not the mechanism. Argument
// inference reports the mismatch before unification is consulted, so
// `Unifier::without_subtype_widening` — the guard that would refuse it a second
// time — is belt-and-braces rather than the layer that decides this program.
// expect-error: got `(arg0: Sub) => number`
class Base { x: number = 1; }
class Sub extends Base { y: number = 2; }

function apply<T>(v: T, f: (a: T) => number): number {
  return f(v);
}

function main(): void {
  const s: Sub = new Sub();
  const n = apply<Base>(s, (a: Sub): number => a.y);
}
