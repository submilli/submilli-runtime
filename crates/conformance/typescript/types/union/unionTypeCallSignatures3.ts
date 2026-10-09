// @target: es2015
function f1(s: string): void { }
function f2(s?: string): void { }
function f3(...s: string[]): void { }
function f4(s: string, s2?: string): void { }
function f5(s?: string, n?: number): void { }
function f6(s?: string, ...n: number[]): void { }
function f7(s: string, ...sRest: string[]): void { }

let fUnion: typeof f1 | typeof f2 | typeof f3 | typeof f4 | typeof f5 | typeof f6 | typeof f7 = null as unknown as (typeof f1 | typeof f2 | typeof f3 | typeof f4 | typeof f5 | typeof f6 | typeof f7);

fUnion(""); // All constituents can be called by passing a single string.


function main(): void {}
