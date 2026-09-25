class AccessorBase {
  z: number = 1;
  get g(): number { throw new Error('enumeration must not invoke getters'); }
  set g(value: number) {}
}
class AccessorChild extends AccessorBase { x: number = 2; }
class DataKey { 'get g': () => number = (): number => 3; }
class CollidingKeys { 'get g': () => number = (): number => 9; get g(): number { return 4; } }
function main(): void {
  const base = new AccessorBase();
  assert(Object.keys(base).join(',') === 'z', 'keys hide accessor slots');
  assert(Object.values(base).length === 1, 'values hide accessor slots');
  assert(Object.entries(base).length === 1, 'entries hide accessor slots');
  const erased: unknown = base;
  const view = erased as { z: number };
  assert('g' in view, 'in sees accessor property');
  assert(!('get g' in view), 'in hides getter slot');
  assert(!('set g' in view), 'in hides setter slot');
  assert(!Object.hasOwn(base, 'get g'), 'hasOwn hides getter slot');
  assert(!Object.hasOwn(base, 'g'), 'accessors belong to prototype');
  const child = new AccessorChild();
  assert(Object.keys(child).length === 2, 'inherited accessor slots hidden');
  const data = { 'get g': 3, 'set g': 4, call: (): number => 5 };
  assert(Object.keys(data).length === 3, 'ordinary data and closure fields remain enumerable');
  assert('get g' in data, 'ordinary getter-like key remains present');
  assert('set g' in data, 'ordinary setter-like key remains present');
  assert(Object.hasOwn(data, 'get g'), 'ordinary key remains own');
  const dataClass = new DataKey();
  assert(Object.keys(dataClass).join(',') === 'get g', 'class data slot remains enumerable');
  assert('get g' in dataClass, 'class data getter-like key remains visible');
  assert(!('g' in dataClass), 'class data slot is not an accessor');
  const collision = new CollidingKeys();
  assert('get g' in collision, 'data name can coexist with accessor');
  assert('g' in collision, 'accessor remains present beside colliding data name');
  const collisionView = collision as unknown as { g: number };
  assert(collisionView.g === 4, 'shaped read selects accessor namespace');
}
