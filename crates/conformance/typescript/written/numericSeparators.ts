// Written for Submilli: none of TypeScript's own tests that Submilli can run
// uses a numeric separator (`1_000`).

let million = 1_000_000;
let fraction: number = 1_000.000_5;
let exponent = 1e1_0 + 2.5e-1_0;
let radix = 0xFF_FF + 0b1010_1010 + 0o7_7;
let big: bigint = 1_000n * 0xF_Fn;
let wrong: string = 1_000;
let trailing = 1_;
let doubled = 1__000;
let afterZero = 0_1;

function main(): void {}
