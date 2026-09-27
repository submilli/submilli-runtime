// @target: es2015
// @strict: true
interface Pair<T1, T2> { first: T1; second: T2; }
let x: Pair<string, number> = null as unknown as (Pair<string, number>);
let y: { first: string; second: number; } = null as unknown as ({ first: string; second: number; });

x = y;
y = x;

function f<T, U>(x: Pair<T, U>): void { }
function f2<T, U>(x: { first: T; second: U; }): void { }

f(x);
f(y);
f2(x);
f2(y);

function main(): void {}
