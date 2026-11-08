// @target: es2015
enum E { a, b, c }

let a: E = null as unknown as (E);
let b: number = null as unknown as (number);

// operator <
let ra1 = a < b;
let ra2 = b < a;
let ra3 = E.a < b;
let ra4 = b < E.a;
let ra5 = E.a < 0;
let ra6 = 0 < E.a;

// operator >
let rb1 = a > b;
let rb2 = b > a;
let rb3 = E.a > b;
let rb4 = b > E.a;
let rb5 = E.a > 0;
let rb6 = 0 > E.a;

// operator <=
let rc1 = a <= b;
let rc2 = b <= a;
let rc3 = E.a <= b;
let rc4 = b <= E.a;
let rc5 = E.a <= 0;
let rc6 = 0 <= E.a;

// operator >=
let rd1 = a >= b;
let rd2 = b >= a;
let rd3 = E.a >= b;
let rd4 = b >= E.a;
let rd5 = E.a >= 0;
let rd6 = 0 >= E.a;

// operator ==
let re1 = a == b;
let re2 = b == a;
let re3 = E.a == b;
let re4 = b == E.a;
let re5 = E.a == 0;
let re6 = 0 == E.a;

// operator !=
let rf1 = a != b;
let rf2 = b != a;
let rf3 = E.a != b;
let rf4 = b != E.a;
let rf5 = E.a != 0;
let rf6 = 0 != E.a;

// operator ===
let rg1 = a === b;
let rg2 = b === a;
let rg3 = E.a === b;
let rg4 = b === E.a;
let rg5 = E.a === 0;
let rg6 = 0 === E.a;

// operator !==
let rh1 = a !== b;
let rh2 = b !== a;
let rh3 = E.a !== b;
let rh4 = b !== E.a;
let rh5 = E.a !== 0;
let rh6 = 0 !== E.a;

function main(): void {}
