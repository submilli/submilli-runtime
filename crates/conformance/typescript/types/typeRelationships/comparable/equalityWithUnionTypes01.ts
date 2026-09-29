// @target: es2015
interface I1 {
    p1: number
}

interface I2 extends I1 {
    p2: number;
}

let x = { p1: 10, p2: 20 };
let y: number | I2 = x;
let z: I1 = x;

if (y === z || z === y) {
}
else if (y !== z || z !== y) {
}
else if (y == z || z == y) {
}
else if (y != z || z != y) {
}

function main(): void {}
