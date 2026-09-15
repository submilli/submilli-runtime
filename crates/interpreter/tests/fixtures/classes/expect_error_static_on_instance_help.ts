// The help has to be code that parses: a static is reached through the bare class
// name, so a generic receiver must not render its type arguments there.
// expect-error: is a static member of `Box<number>`
// expect-error: write `Box.kind`
class Box<T> {
  static kind: string = "box";
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}

function main(): void {
  const b = new Box<number>(1);
  console.log(b.kind);
}
