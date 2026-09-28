class Counter {
  private current: number = 1;
  get value(): number { return this.current; }
  set value(next: number) { this.current = next; }
}
class ReadOnly {
  get value(): number { return 3; }
}
function main(): void {
  const counter: Record<string, number> = new Counter();
  const key: string = "value";
  assert(counter[key] === 1);
  counter[key] = 7;
  assert(counter.value === 7);
  assert(key in counter);
  const readonly = new ReadOnly() as Record<string, number>;
  assert(readonly[key] === 3);
  let caught = false;
  try { readonly[key] = 5; } catch (e: Error) { caught = true; }
  assert(caught);
}
