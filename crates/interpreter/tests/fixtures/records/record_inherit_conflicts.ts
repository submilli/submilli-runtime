// expect-error: incompatible inherited member
interface A { [key: string]: number; x: 1; }
interface B { [key: string]: number; x: number; }
interface BadTypes extends A, B {}
interface Mutable { [key: string]: number; x: number; }
interface Immutable { [key: string]: number; readonly x: number; }
interface BadReadonly extends Mutable, Immutable {}
interface BadReverse extends Immutable, Mutable {}
interface MethodBase { [key: string]: unknown; read(): number; }
interface ScalarBase { [key: string]: unknown; read: number; }
interface BadKinds extends MethodBase, ScalarBase {}
interface BadOverride extends MethodBase { read: number; }
function main(): void {}
