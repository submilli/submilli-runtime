// @target: es2015
// Generic typed parameters with initializers

function foo<T>(x: T = null): T { return x; } // ok
function foo2<T>(x: T = undefined): T { return x; } // ok
function foo3<T extends Number>(x: T = 1): void { } // error
function foo4<T, U extends T>(x: T, y: U = x): void { } // error
function foo5<T, U extends T>(x: U, y: T = x): void { } // ok
function foo6<T, U extends T, V extends U>(x: T, y: U, z: V = y): void { } // error
function foo7<T, U extends T, V extends U>(x: V, y: U = x): void { } // should be ok

function main(): void {}
