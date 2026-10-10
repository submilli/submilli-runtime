// expect-error: cannot read `y` on a value of type `Inner | null`
// expect-error: cannot read `y` on a value of type `Nil | Inner`
// expect-error: cannot read `y` on a value of type `null` — the receiver is always null
// `?.` short-circuits its own step only. A plain `.` after a step that yields a
// nullable value is an ordinary field read on a nullable receiver, and is
// rejected exactly as it is outside a chain — admitting it emits a `struct.get`
// against a value that can be null.
//
// The second case pins that the nullability test peels union members: `Nil` is
// an alias for `null`, so a nominal test would miss it and admit the step.
type Nil = null;

class Inner {
  y: number = 2;
}

class Outer {
  b: Inner | null = null;
  aliased: Inner | Nil = null;
}

// A step is admitted on the strength of a narrowing, so the *absence* of one is
// often the whole reason it is rejected. The diagnostic names what killed it,
// as the plain read does — bare `?.y` advice would silence the write rather
// than fix it.
function droppedNarrowing(): void {
  const w = new Outer();
  if (w.b !== null) {
    w.b = null;
    const r = w?.b.y;
    const optional = w?.b?.y;
    console.log(r);
  }
}

function main(): void {
  const a: Outer | null = new Outer();
  const v = a?.b.y;
  const w = a?.aliased.y;
  console.log(v, w);
}
