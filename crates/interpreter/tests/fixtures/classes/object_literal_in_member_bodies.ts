// An object literal whose shape appears only inside a class member body still
// gets registered: class bodies are shape-collection roots, so codegen has a
// vtable global for it.
class Reporter {
  private tag: string = "r";

  build(): { alpha: number; beta: string } {
    return { alpha: 1, beta: this.tag };
  }

  nested(): number {
    const outer = { inner: { depth: 2 }, label: "x" };
    return outer.inner.depth;
  }

  get summary(): { count: number } {
    return { count: 7 };
  }

  static origin(): { where: string } {
    return { where: "static" };
  }

  // Reached only through vtable dispatch from `JSON.stringify`, so this body's
  // literal has no direct call site to register its shape.
  toJson(): string {
    const payload = { gamma: true };
    return JSON.stringify(payload);
  }
}

function main(): void {
  const r = new Reporter();
  const built = r.build();
  assert(built.alpha === 1 && built.beta === "r", "literal shape from a method body");
  assert(r.nested() === 2, "nested literal shape from a method body");
  assert(r.summary.count === 7, "literal shape from an accessor body");
  assert(Reporter.origin().where === "static", "literal shape from a static method body");
  assert(JSON.stringify(r) === "{\"gamma\":true}", "literal shape from a vtable-dispatched toJson");
}
