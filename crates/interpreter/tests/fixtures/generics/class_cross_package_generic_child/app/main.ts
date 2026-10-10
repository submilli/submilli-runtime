import { Box } from "@t/lib";

class Tagged<T> extends Box<T> {
  readonly tag: string;
  constructor(v: T, tag: string) { super(v); this.tag = tag; }
  labelled(): string { return this.tag + "!"; }
}

function main(): void {
  const t = new Tagged("v", "s");
  assert(t.get() === "v", "generic child of an imported generic parent");
  assert(t.labelled() === "s!", "child method");
  const asBox: Box<string> = t;
  assert(asBox.get() === "v", "parent-typed dispatch");
  assert(asBox.pair("a", "b") === "a", "two erased args across the boundary");
  const n = new Tagged(1, "n");
  assert(n.get() === 1, "second instantiation of the local generic child");
}
