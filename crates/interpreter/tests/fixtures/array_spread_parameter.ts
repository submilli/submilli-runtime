// Spreading a parameter into an array literal (SUB-1068). Codegen may widen a
// parameter's value to `unknown`, and the spread must still read it as an array.
function copy(xs: number[]): number[] {
  return [...xs];
}

function wrap(xs: string[]): string[] {
  return ["<", ...xs, ">"];
}

function fromTuple(t: [number, string]): number {
  return [...t].length;
}

function fromReadonly(xs: readonly number[]): number {
  return [...xs].length;
}

class Bag {
  items: number[] = [1, 2];

  copy(xs: number[]): number[] {
    return [...xs];
  }

  own(): number[] {
    return [...this.items];
  }
}

type Holder = { v: number[] | null };

function clear(h: Holder): void {
  h.v = null;
}

// The narrowing of `h.v` is stale once `clear` runs, so the spread must throw
// a catchable error, as an index read of it does, rather than trap.
function spreadStale(h: Holder): number {
  if (h.v !== null) {
    clear(h);
    try {
      return [...h.v].length;
    } catch (e) {
      return -1;
    }
  }
  return 0;
}

function main(): void {
  const source = [3, 1, 2];
  const copied = copy(source);
  source.push(4);
  assert(copied.length === 3, "the copy doesn't share the parameter's storage");
  assert(copied.join(",") === "3,1,2", "the copy keeps the order");
  assert(wrap(["a", "b"]).join("") === "<ab>", "spread between elements");
  assert(fromTuple([1, "a"]) === 2, "a tuple parameter spreads");
  assert(fromReadonly([1, 2]) === 2, "a readonly parameter spreads");
  const bag = new Bag();
  assert(bag.copy([4, 5, 6]).length === 3, "a method parameter spreads");
  assert(bag.own().join(",") === "1,2", "a field spreads");
  const escaped = copy;
  assert(escaped([7]).length === 1, "a function used as a value spreads its parameter");
  assert(spreadStale({ v: [1] }) === -1, "a stale narrowed source throws a catchable error");
}
