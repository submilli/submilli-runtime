// expect-error: incompatible inherited member `id`
// expect-error: incompatible inherited member `read`
interface Identity { [key: string]: unknown; id<T>(value: T): T; }
interface ConcreteOverride extends Identity { id(value: number): number; }
interface Concrete { id(value: number): number; }
interface BadBases extends Identity, Concrete {}
interface Unused { id<T>(value: number): number; }
interface UnusedMismatch extends Unused, Concrete {}
interface Factory { [key: string]: unknown; read<T>(): T; }
interface ConcreteFactory extends Factory { read<T>(): number; }
interface Independent { id<T, U>(value: T): U; }
interface BadRelationship extends Independent { id<T>(value: T): T; }
function main(): void {}
// expect-error: property `identity`
interface BadIndex { [key: string]: (value: number) => string; identity<T>(value: T): T; }
