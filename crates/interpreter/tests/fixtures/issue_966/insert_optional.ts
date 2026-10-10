interface Value { a?: string | null; b?: number }
function main(): void {
  const raw: unknown = JSON.parse('{}');
  const parsed = raw as Value;
  const alias = raw as Value;
  parsed.a = null;
  assert('a' in alias, 'insertion visible through alias');
  const present = 'a' in alias;
  assert(present === true, 'inserted presence is a canonical boolean');
  assert(present !== false, 'inserted presence differs from false');
  assert(JSON.stringify(raw) === '{"a":null}', 'null inserted');
  parsed.a = 'hello';
  assert(alias.a === 'hello', 'overwrite inserted slot');
  parsed.b = 2;
  assert(alias.b === 2, 'second insertion');
  assert(Object.keys(parsed).join(',') === 'a,b', 'inserted keys enumerable');
  const original = { z: 1 };
  const widened: { z: number; a?: string } = original;
  widened.a = 'new';
  assert('a' in original, 'structurally widened object keeps identity');
  assert(original.z === 1, 'existing field survives growth');
}
