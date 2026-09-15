// Generic classes in nullable unions: null-check narrowing on results and on
// a `T | null` field inside the class.
class Slot<T> {
  private current: T | null;
  constructor() {
    this.current = null;
  }
  put(v: T): void {
    this.current = v;
  }
  take(): T | null {
    const out = this.current;
    this.current = null;
    return out;
  }
}

function find(key: string): Slot<number> | null {
  if (key === "hit") {
    const s = new Slot<number>();
    s.put(99);
    return s;
  }
  return null;
}

function main(): void {
  const hit = find("hit");
  if (hit === null) {
    assert(false, "find(\"hit\") returned a slot");
    return;
  }
  const v = hit.take();
  assert(v !== null && v === 99, "erased T | null return narrows");
  assert(hit.take() === null, "emptied");

  const miss = find("miss");
  assert(miss === null, "not found");
}
