// test262: test/built-ins/Object/keys/15.2.3.14-3-1.js
// The 'x' < 'y' insertion order coincides with our canonical sorted order,
// so the original assertions hold verbatim.

function main(): void {
  const o = {
    x: 1,
    y: 2,
  };

  const a: string[] = Object.keys(o);

  assertSameValue(a.length, 2, "a.length");
  assertSameValue(a[0], "x", "a[0]");
  assertSameValue(a[1], "y", "a[1]");
}
