class K { v: number = 1; }
class Child extends K {}
class Other { v: number = 2; }
type Identity<T> = T;
function direct<T>(x: T): boolean { return x instanceof K; }
function alias<T>(x: Identity<T>): boolean { return x instanceof K; }
function union<T>(x: T | number | null): boolean { return x instanceof K; }
export function main(): void {
  assert(direct<K>(new K()), "erased class instance");
  assert(direct<Child>(new Child()), "erased subclass");
  assert(!direct<Other>(new Other()), "nominal identity");
  assert(!direct<number>(1), "erased primitive");
  assert(!direct<null>(null), "erased null");
  assert(alias<K>(new K()), "alias of erased operand");
  assert(!alias<string>("no"), "alias primitive");
  assert(union<K>(new K()), "union erased member");
  assert(!union<K>(3), "union concrete member");
  assert(!union<K>(null), "union null member");
}
