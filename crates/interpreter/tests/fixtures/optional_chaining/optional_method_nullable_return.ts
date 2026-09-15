// `a?.m()` where `m` itself returns a nullable type. The chain part's
// `result_ty` is the method's *declared* return; the `| null` an optional step
// contributes comes from the enclosing null-check, not from the part. Stripping
// null off the declared return would discard a null the method genuinely
// returns, and the return cast would then `ref.as_non_null` that very value.
//
// TypeScript semantics: a null *return* flows out as null, exactly like a null
// receiver short-circuits — neither is an error.
class Leafy {
  constructor(public tag: string) {}
  at(i: number): Leafy | null {
    return i > 0 ? null : this;
  }
  label(): string {
    return this.tag;
  }
  touch(): void {}
}

interface Finder {
  find(k: string): string | null;
}

class Store implements Finder {
  find(k: string): string | null {
    return k === "hit" ? "found" : null;
  }
}

function main(): void {
  const root: Leafy | null = new Leafy("r");

  // the method returns null — the chain yields null rather than trapping
  assert(root?.at(5) === null, "nullable return through an optional call");
  // ...and the same when a further optional step follows
  assert(root?.at(5)?.tag === null, "optional step after a null-returning call");
  // the non-null path still reaches the value
  assert(root?.at(0)?.tag === "r", "non-null return continues the chain");

  // a non-nullable return is unaffected
  assert(root?.label() === "r", "non-nullable return");
  // void method through an optional chain
  root?.touch();

  // same through an interface-typed receiver, which dispatches by shape
  const store: Finder | null = new Store();
  assert(store?.find("miss") === null, "nullable return, interface receiver");
  assert(store?.find("hit") === "found", "non-null return, interface receiver");

  // a null receiver short-circuits regardless
  const gone: Leafy | null = null;
  assert(gone?.at(0) === null, "null receiver short-circuits");
  assert(gone?.at(0)?.tag === null, "null receiver, two steps");
}
