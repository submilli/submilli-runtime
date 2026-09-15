// expect-error: is write-only
// Reading a write-only accessor (setter, no getter) is a compile error.
class Sink {
  private buf: string = "";
  set data(v: string) {
    this.buf = v;
  }
}

function main(): void {
  const s = new Sink();
  s.data = "x";
  const r: string = s.data;
  assert(r === "x");
}
