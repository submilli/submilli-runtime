// expect-error: method `toUpperCase` must be called
// expect-error: method `m` must be called
// expect-error: method `describe` must be called
// expect-error: method `trim` must be called
// expect-error: method `slice` must be called
// expect-error: method `render` must be called
// expect-error: method `toJson` must be called
// A bare instance-method reference is not a value — `s.toUpperCase` without a
// call is rejected off a plain receiver, so `?.` must reject it too.

interface Describable {
  describe(): string;
}

class Widget implements Describable {
  m(): string {
    return "m";
  }
  render(): string {
    return "rendered";
  }
  describe(): string {
    return "widget";
  }
}

function main(): void {
  const s: string | null = "ab";
  const bare = s?.toUpperCase;

  const w: Widget | null = new Widget();
  const viaClass = w?.m;

  const d: Describable | null = new Widget();
  const viaInterface = d?.describe;

  // Mid-chain: still never called, so still rejected — and the poisoned
  // receiver keeps `.length` from reporting a miss on the function type.
  const midChain = s?.trim.length;

  // The plain-receiver form is the same defect and reports the same way — on a
  // primitive, on a nominal type, and on an object literal, whose members
  // resolve against the prelude `Object` interface.
  const plainPrimitive = "ab".slice;
  const plainClass = new Widget().render;
  const plainObjectLiteral = { a: 1 }.toJson;

  assert(bare === null || viaClass === null || viaInterface === null || midChain === null, "unreachable");
}
