// An assignment can be used as a value. It yields the assigned value, and it
// is checked, narrowed, and evaluated exactly as the assignment statement is.

let global = 0;

class Temperature {
  private c: number = 0;
  static count: number = 0;
  get celsius(): number {
    return this.c;
  }
  set celsius(v: number) {
    this.c = Math.round(v);
  }
}

let calls = 0;

function counted(o: { f: number }): { f: number } {
  calls = calls + 1;
  return o;
}

let position = 0;
const items = [1, 2, 3];

function next(): number | null {
  if (position < items.length) {
    const item = items[position];
    position++;
    return item;
  }
  return null;
}

function main(): void {
  let a = 0;
  let b = 0;
  b = (a = 3);
  assert(a === 3 && b === 3, "the value of an assignment is the assigned value");
  a = b = 5;
  assert(a === 5 && b === 5, "assignment chains right to left");
  assert((a += 2) === 7 && a === 7, "a compound assignment yields the new value");
  (a) = 1;
  assert(a === 1, "a parenthesized target is assigned");

  // The loop idiom: the assignment narrows through the comparison around it.
  let item: number | null = null;
  let total = 0;
  while ((item = next()) !== null) {
    total += item;
  }
  assert(total === 6, "while ((x = next()) !== null) reads each item");

  let text: string | null = null;
  const assigned = (text = "hi");
  assert(assigned.length === 2 && text.length === 2, "the write narrows the binding");

  // A field or index target's receiver is evaluated once, and the result is
  // the value assigned, not a read back through the target.
  const box = { f: 1 };
  assert((counted(box).f = 7) === 7 && box.f === 7 && calls === 1, "field target");
  assert((counted(box).f *= 2) === 14 && box.f === 14 && calls === 2, "compound field target");
  const list = [1, 2, 3];
  assert((list[1] += 5) === 7 && list.join(",") === "1,7,3", "index target");
  const bytes = new Uint8Array(1);
  const stored = (bytes[0] = 300);
  assert(stored === 300 && bytes.join(",") === "44", "the value is not read back");
  const t = new Temperature();
  const set = (t.celsius = 2.6);
  assert(set === 2.6 && Math.round(set) === 3, "a setter's result is not read back");
  assert(t.celsius + 0 === 3, "the setter ran");

  assert((global = 3) === 3 && global === 3, "a global target");
  assert((Temperature.count += 4) === 4, "a static field target");

  let captured = 0;
  const read = (): number => captured;
  const write = (): number => (captured = 9);
  assert((captured = 5) === 5 && read() === 5, "a captured binding is written");
  assert(write() === 9 && captured === 9, "an arrow body can be an assignment");

  let s = "a";
  assert(`${(s += "b")}` === "ab", "inside a template");
  let n = 0;
  assert([(n = 1), (n = n + 1)].join(",") === "1,2", "array elements run in order");
  let i = 0;
  let sum = 0;
  for (let step = 0; (step = step + 1) < 4; ) {
    sum += step;
  }
  assert(sum === 6, "in a for condition");
  i = (i = 2) + i;
  assert(i === 4, "the target is read after the inner write");
}
