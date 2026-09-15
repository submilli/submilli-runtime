// test262: test/built-ins/Object/hasOwn/hasown_nonexistent.js

function main(): void {
  const o = {};

  assertSameValue(Object.hasOwn(o, "foo"), false, 'Object.hasOwn(o, "foo")');
}
