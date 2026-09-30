// Adapted by hand from the upstream case. Parameters and functions are typed,
// since Submilli requires it. Removed, since Submilli supports none of them:
// `declare function` (an ambient one and the overloads), the spread of an empty
// array into a call, a `<T>` cast of an overloaded call, and the interface of
// call and construct signatures. The trailing comma after a rest parameter
// stays: both reject it. Added: `made`, to construct `X` and write through its
// setter. `@strict: false` is gone, since Submilli is always strict.
// @target: es5, es2015

function f1(x: number,): void {}

f1(1,);

function f2(...args: number[],): void {}


// Works for constructors too
class X {
    constructor(a: number,) { }
    set x(value: number,) { }
}

let made: X = new X(1,);
made.x = 2;

function main(): void {}
