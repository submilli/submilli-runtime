// @target: es2015
enum E { a, b }
enum F { c, d }

let a: number = null as unknown as (number);
let b: E = null as unknown as (E);
let c: E | F = null as unknown as (E | F);

let r1 = a + a;
let r2 = a + b;
let r3 = b + a;
let r4 = b + b;

let r5 = 0 + a;
let r6 = E.a + 0;
let r7 = E.a + E.b;
let r8 = E['a'] + E['b'];
let r9 = E['a'] + F['c'];

let r10 = a + c;
let r11 = c + a;
let r12 = b + c;
let r13 = c + b;
let r14 = c + c;


function main(): void {}
