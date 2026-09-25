class Base { value: number[] | string | null = null; }
class Child extends Base { value: number[] = [1]; }
function main(): void {
  const child = new Child();
  const optional = child as unknown as { extra?: number };
  optional.extra = 5;
  const parent: Base = child;
  parent.value = 'wrong';
  let caught = false;
  try { child.value.length; } catch (error: Error) { caught = error instanceof TypeError; }
  assert(caught, 'guard still rejects parent write after insertion');
}
