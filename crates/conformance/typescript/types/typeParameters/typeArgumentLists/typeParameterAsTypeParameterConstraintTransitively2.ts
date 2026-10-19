// @target: es2015
// @strict: true
// using a type parameter as a constraint for a type parameter is invalid
// these should be errors at the type parameter constraint declarations, and have no downstream errors

interface A { foo: number }
interface B extends A { bar: string; }
interface C extends B { baz: boolean; }
let a: A = null as unknown as (A);
let b: B = null as unknown as (B);
let c: C = null as unknown as (C);

function foo<T, U, V>(x: T, y: U, z: V): V { return z; }
//function foo<T, U extends T, V extends U>(x: T, y: U, z: V): V { return z; }

foo(1, 2, '');
foo({ x: 1 }, { x: 1, y: '' }, { x: 2, y: 2, z: true });
foo(a, b, a);
foo(a, { foo: 1, bar: '', hm: true }, b);
/*pruned*/;                                                           

/*pruned*/;                                                                            
//function foo2<T extends A, U extends T, V extends U>(x: T, y: U, z: V): V { return z; }
foo(b, a, c);
foo(c, c, a);

function main(): void {}
