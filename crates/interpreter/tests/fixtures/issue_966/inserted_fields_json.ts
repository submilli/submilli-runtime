class Data { a: number = 1; }
class Empty {}
class Parent { z: number[] | string | null = null; }
class Guarded extends Parent { z: number[] = [1]; }
class AccessorData { 'get g': number = 9; get g(): number { return 4; } }
function main(): void {
  const data = new Data();
  const extension = data as unknown as { b?: number };
  extension.b = 2;
  assert(JSON.stringify(data) === '{"a":1,"b":2}', 'class inserted field serialized');
  const empty = new Empty();
  const emptyView = empty as unknown as { a?: number | null };
  emptyView.a = null;
  assert(JSON.stringify(empty) === '{"a":null}', 'empty class inserted null serialized');
  const literal = { a: 1 };
  const literalView: { a: number; b?: number } = literal;
  literalView.b = 2;
  literalView.b = 3;
  literalView.b = 2;
  assert(JSON.stringify(literal) === '{"a":1,"b":2}', 'literal inserted field serialized');
  assert(JSON.stringify({ nested: literal }) === '{"nested":{"a":1,"b":2}}', 'nested inserted field serialized');
  const withFunction = { a: 1, callback: (): number => 3 };
  const functionView: { a: number; b?: number } = withFunction;
  functionView.b = 2;
  assert(JSON.stringify(withFunction) === '{"a":1,"b":2}', 'fallback serializer includes inserted data');
  const guarded = new Guarded();
  const guardView = guarded as unknown as { a?: number };
  guardView.a = 2;
  const guardJson = JSON.parse(JSON.stringify(guarded)) as { a: number; z: number[] };
  assert(guardJson.a === 2 && guardJson.z[0] === 1 && Object.keys(guardJson).length === 2, 'guard tail hidden and inserted data serialized');
  const accessorData = new AccessorData();
  const accessorView = accessorData as unknown as { b?: number };
  accessorView.b = 2;
  const accessorJson = JSON.parse(JSON.stringify(accessorData)) as { b: number; g: number; 'get g': number };
  assert(accessorJson.b === 2 && accessorJson['get g'] === 9 && accessorJson.g === 4 && Object.keys(accessorJson).length === 3, 'dynamic serializer preserves colliding data key and public getter');
}
