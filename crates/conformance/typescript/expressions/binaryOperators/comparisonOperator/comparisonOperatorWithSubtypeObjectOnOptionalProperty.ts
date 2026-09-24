// @target: es2015
interface I {
    a: string;
    b?: number;
}

interface J {
    a: string;
}

let a: I = null as unknown as (I);
let b: J = null as unknown as (J);

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
