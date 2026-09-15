// test262: test/built-ins/Object/hasOwn/hasown_own_property_exists.js

function main(): void {
  const o = {
    foo: 42,
  };

  assertSameValue(Object.hasOwn(o, "foo"), true, 'Object.hasOwn(o, "foo") !== true');
}
