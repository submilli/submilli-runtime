// @target: es2015
// @strict: true
interface I1 {
    p1: number
}

interface I2 extends I1 {
    p2: number;
}

let x = { p1: 10, p2: 20 };
let y: number | I2 = x;
let z: I1 = x;

let a = <number | I2>z;
let b = <number>z;
let c = <I2>z;
let d = <I1>y;


function main(): void {}
