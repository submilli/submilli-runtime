// expect-error: expected `"A" | "B"`, got `"C"`
// `typeof x.p` on an instantiated generic interface is the field's type with
// the interface's type arguments, not its bare type parameter, narrowed where
// `x.p` is.
interface Box<T> {
  p: T;
}

function onlyA(value: "A"): void {}

function pick(x: Box<"A" | "B">): void {
  const ok: typeof x.p = "A";
  let y: typeof x.p = "C";
  if (x.p === "A") {
    const narrowed: typeof x.p = x.p;
    onlyA(narrowed);
  }
}

function main(): void {}
