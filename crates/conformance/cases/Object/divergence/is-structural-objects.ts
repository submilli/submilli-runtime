// Not a test262 port: pins our documented divergence from JS SameValue on
// objects. test262's is/not-same-value-x-y-object.js expects
// `Object.is({}, {})` to be `false` (reference identity); our Object.is
// follows the language's structural `===`, so structurally equal objects
// match. The original lives under rejected/Object/is/.

function main(): void {
  const a = { x: 1 };
  const b = { x: 1 };
  const c = { x: 2 };

  assertSameValue(Object.is(a, b), true, "structurally equal objects match");
  assertSameValue(Object.is(a, c), false, "structurally distinct objects do not");

  const e1 = {};
  const e2 = {};
  assertSameValue(Object.is(e1, e2), true, "two empty objects match");
}
