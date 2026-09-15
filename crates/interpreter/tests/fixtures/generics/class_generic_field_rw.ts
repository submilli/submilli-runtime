// Generic fields read/write from outside and via `this`, plus a T-typed
// accessor pair (erased getter/setter vtable slots).
class Cell<T> {
  value: T;
  private backing: T;
  constructor(v: T) {
    this.value = v;
    this.backing = v;
  }
  refresh(): void {
    this.value = this.backing;
  }
  get shadow(): T {
    return this.backing;
  }
  set shadow(v: T) {
    this.backing = v;
  }
}

function main(): void {
  const c = new Cell("abc");
  assert(c.value.length === 3, "unboxed field read feeds string method");
  c.value = "wxyz";
  assert(c.value === "wxyz", "external field write");
  c.refresh();
  assert(c.value === "abc", "this-write inside method");

  const n = new Cell(5);
  n.value = n.value + 1;
  assert(n.value === 6, "number field arithmetic");
  n.shadow = 50;
  assert(n.shadow === 50, "T-typed accessor round-trip");
  n.refresh();
  assert(n.value === 50, "accessor wrote the backing field");
}
