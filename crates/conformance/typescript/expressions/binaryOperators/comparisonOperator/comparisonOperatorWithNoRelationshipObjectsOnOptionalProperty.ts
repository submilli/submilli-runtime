// @target: es2015
interface A1 {
    b?: number;
}

interface B1 {
    b?: string;
}

let a: A1 = null as unknown as (A1);
let b: B1 = null as unknown as (B1);

// operator <
let ra1 = a < b;
let ra2 = b < a;

// operator >
let rb1 = a > b;
let rb2 = b > a;

// operator <=
let rc1 = a <= b;
let rc2 = b <= a;

// operator >=
let rd1 = a >= b;
let rd2 = b >= a;

// operator ==
let re1 = a == b;
let re2 = b == a;

// operator !=
let rf1 = a != b;
let rf2 = b != a;

// operator ===
let rg1 = a === b;
let rg2 = b === a;

// operator !==
let rh1 = a !== b;
let rh2 = b !== a;

function main(): void {}
