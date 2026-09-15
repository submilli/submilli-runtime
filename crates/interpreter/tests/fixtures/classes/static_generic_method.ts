class Box {
  static wrap<T>(x: T): T[] {
    return [x];
  }
}

function main(): void {
  const a = Box.wrap<number>(7);
  assert(a.length === 1);
  assert(a[0] === 7);
  const s = Box.wrap("hi");
  assert(s[0] === "hi");
}
