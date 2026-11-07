// Adapted from TypeScript: distinct names for repeated var bindings, initialized
// interface values, explicit return types, and a main entry point.
// @target: es2015
// @strict: true
function foo<T, U extends T>(x: T, y: U): U { return y; }
const r = foo(1, 2);
const r1 = foo({}, 1);
interface A { foo: string; }
interface B extends A { bar: number; }
const a: A = { foo: "a" };
const b: B = { foo: "b", bar: 1 };
const r2 = foo(a, b);
const r3 = foo({ x: 1 }, { x: 2, y: 3 });
function foo2<T, U extends { length: T }>(x: T, y: U): U { return y; }
foo2(1, "");
foo2({}, { length: 2 });
foo2(1, { width: 3, length: 2 });
foo2(1, []);
foo2(1, [""]);
function main(): void {}
