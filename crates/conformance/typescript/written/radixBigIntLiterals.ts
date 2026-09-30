// Written for Submilli: no upstream case Submilli can run writes a `bigint` with
// a radix prefix.

let hex: bigint = 0xffn;
let binary = 0b1010n;
let octal = 0o17n + 1n;
let mixed = 0XFFn * 0B11n - 0O7n;
let wrong: number = 0x10n;

function main(): void {}
