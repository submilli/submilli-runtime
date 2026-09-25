class Plain { value: number = 1; }
class Base { value: number[] | string | null = null; }
class Child extends Base { value: number[] = [1]; }
class Box<T> { value: T; constructor(value: T) { this.value = value; } get(): T { return this.value; } }
function extend(target: { extra?: number }): number {
  target.extra = 7;
  return 9;
}
function main(): void {
  const object: { value: number; extra?: number } = { value: 1 };
  object.value = extend(object);
  assert(object.value === 9 && object.extra === 7, 'structural store uses grown payload');
  const instance = new Plain();
  instance.value = extend(instance as unknown as { extra?: number });
  assert(instance.value === 9, 'class store uses grown payload');
  const child = new Child();
  const optional = child as unknown as { value: number[]; extra?: number };
  optional.extra = 5;
  assert(child.value.length === 1, 'guard still reads after insertion');
  const box = new Box<number>(3);
  const extension = box as unknown as { extra?: number; more?: number };
  extension.extra = 4;
  extension.more = 5;
  assert(box.get() === 3, 'generic context survives multiple insertions');
}
