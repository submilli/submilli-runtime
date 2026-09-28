interface Base { [key: string]: number; x: number; }
interface Child extends Base { readonly x: number; }
interface MethodBase { [key: string]: unknown; read(first: number): number; }
interface OtherMethodBase { [key: string]: unknown; read(second: number): number; }
interface PropertyBase { [key: string]: unknown; read: (input: number) => number; }
interface Combined extends MethodBase, OtherMethodBase, PropertyBase {}
interface Reversed extends PropertyBase, OtherMethodBase, MethodBase {}
interface PropertyOverride extends MethodBase { read: (input: number) => number; }
interface MethodOverride extends PropertyBase { read(input: number): number; }
function main(): void {
  const child: Child = { x: 1 };
  assert(child.x === 1);
  const combined: Combined = { read: (n: number): number => n + 1 };
  assert(combined.read(2) === 3);
  const reversed: Reversed = combined;
  assert(reversed.read(3) === 4);
  const property: PropertyOverride = combined;
  const method: MethodOverride = property;
  assert(method.read(4) === 5);
}
