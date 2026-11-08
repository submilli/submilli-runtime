// Adapted from TypeScript: retain the {} constraint half, add return types and
// initialize the interface value. Object's boxed primitive type is not modeled.
// @target: es2015
// @strict: true
function foo2<T extends {}>(x: T): void {}
const r = foo2({});
const a = {};
const r1 = foo2({});
class C2<T extends {}> {
  constructor(public x: T) {}
}
const r2 = new C2({});
interface I2<T extends {}> { x: T; }
const i2: I2<{}> = { x: {} };
function main(): void {}
