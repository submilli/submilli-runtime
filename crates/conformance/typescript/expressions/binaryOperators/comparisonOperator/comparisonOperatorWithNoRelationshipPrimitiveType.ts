// @target: es2015
enum E { a, b, c }

let a: number = null as unknown as (number);
let b: boolean = null as unknown as (boolean);
let c: string = null as unknown as (string);
let d: void = null as unknown as (void);
let e: E = null as unknown as (E);

// operator <
let r1a1 = a < b;
let r1a1_2 = a < c;
let r1a1_3 = a < d;
let r1a1_4 = a < e; // no error, expected

let r1b1 = b < a;
let r1b1_2 = b < c;
let r1b1_3 = b < d;
let r1b1_4 = b < e;

let r1c1 = c < a;
let r1c1_2 = c < b;
let r1c1_3 = c < d;
let r1c1_4 = c < e;

let r1d1 = d < a;
let r1d1_2 = d < b;
let r1d1_3 = d < c;
let r1d1_4 = d < e;

let r1e1 = e < a; // no error, expected
let r1e1_2 = e < b;
let r1e1_3 = e < c;
let r1e1_4 = e < d;

// operator >
let r2a1 = a > b;
let r2a1_2 = a > c;
let r2a1_3 = a > d;
let r2a1_4 = a > e; // no error, expected

let r2b1 = b > a;
let r2b1_2 = b > c;
let r2b1_3 = b > d;
let r2b1_4 = b > e;

let r2c1 = c > a;
let r2c1_2 = c > b;
let r2c1_3 = c > d;
let r2c1_4 = c > e;

let r2d1 = d > a;
let r2d1_2 = d > b;
let r2d1_3 = d > c;
let r2d1_4 = d > e;

let r2e1 = e > a; // no error, expected
let r2e1_2 = e > b;
let r2e1_3 = e > c;
let r2e1_4 = e > d;

// operator <=
let r3a1 = a <= b;
let r3a1_2 = a <= c;
let r3a1_3 = a <= d;
let r3a1_4 = a <= e; // no error, expected

let r3b1 = b <= a;
let r3b1_2 = b <= c;
let r3b1_3 = b <= d;
let r3b1_4 = b <= e;

let r3c1 = c <= a;
let r3c1_2 = c <= b;
let r3c1_3 = c <= d;
let r3c1_4 = c <= e;

let r3d1 = d <= a;
let r3d1_2 = d <= b;
let r3d1_3 = d <= c;
let r3d1_4 = d <= e;

let r3e1 = e <= a; // no error, expected
let r3e1_2 = e <= b;
let r3e1_3 = e <= c;
let r3e1_4 = e <= d;

// operator >=
let r4a1 = a >= b;
let r4a1_2 = a >= c;
let r4a1_3 = a >= d;
let r4a1_4 = a >= e; // no error, expected

let r4b1 = b >= a;
let r4b1_2 = b >= c;
let r4b1_3 = b >= d;
let r4b1_4 = b >= e;

let r4c1 = c >= a;
let r4c1_2 = c >= b;
let r4c1_3 = c >= d;
let r4c1_4 = c >= e;

let r4d1 = d >= a;
let r4d1_2 = d >= b;
let r4d1_3 = d >= c;
let r4d1_4 = d >= e;

let r4e1 = e >= a; // no error, expected
let r4e1_2 = e >= b;
let r4e1_3 = e >= c;
let r4e1_4 = e >= d;

// operator ==
let r5a1 = a == b;
let r5a1_2 = a == c;
let r5a1_3 = a == d;
let r5a1_4 = a == e; // no error, expected

let r5b1 = b == a;
let r5b1_2 = b == c;
let r5b1_3 = b == d;
let r5b1_4 = b == e;

let r5c1 = c == a;
let r5c1_2 = c == b;
let r5c1_3 = c == d;
let r5c1_4 = c == e;

let r5d1 = d == a;
let r5d1_2 = d == b;
let r5d1_3 = d == c;
let r5d1_4 = d == e;

let r5e1 = e == a; // no error, expected
let r5e1_2 = e == b;
let r5e1_3 = e == c;
let r5e1_4 = e == d;

// operator !=
let r6a1 = a != b;
let r6a1_2 = a != c;
let r6a1_3 = a != d;
let r6a1_4 = a != e; // no error, expected

let r6b1 = b != a;
let r6b1_2 = b != c;
let r6b1_3 = b != d;
let r6b1_4 = b != e;

let r6c1 = c != a;
let r6c1_2 = c != b;
let r6c1_3 = c != d;
let r6c1_4 = c != e;

let r6d1 = d != a;
let r6d1_2 = d != b;
let r6d1_3 = d != c;
let r6d1_4 = d != e;

let r6e1 = e != a; // no error, expected
let r6e1_2 = e != b;
let r6e1_3 = e != c;
let r6e1_4 = e != d;

// operator ===
let r7a1 = a === b;
let r7a1_2 = a === c;
let r7a1_3 = a === d;
let r7a1_4 = a === e; // no error, expected

let r7b1 = b === a;
let r7b1_2 = b === c;
let r7b1_3 = b === d;
let r7b1_4 = b === e;

let r7c1 = c === a;
let r7c1_2 = c === b;
let r7c1_3 = c === d;
let r7c1_4 = c === e;

let r7d1 = d === a;
let r7d1_2 = d === b;
let r7d1_3 = d === c;
let r7d1_4 = d === e;

let r7e1 = e === a; // no error, expected
let r7e1_2 = e === b;
let r7e1_3 = e === c;
let r7e1_4 = e === d;

// operator !==
let r8a1 = a !== b;
let r8a1_2 = a !== c;
let r8a1_3 = a !== d;
let r8a1_4 = a !== e; // no error, expected

let r8b1 = b !== a;
let r8b1_2 = b !== c;
let r8b1_3 = b !== d;
let r8b1_4 = b !== e;

let r8c1 = c !== a;
let r8c1_2 = c !== b;
let r8c1_3 = c !== d;
let r8c1_4 = c !== e;

let r8d1 = d !== a;
let r8d1_2 = d !== b;
let r8d1_3 = d !== c;
let r8d1_4 = d !== e;

let r8e1 = e !== a; // no error, expected
let r8e1_2 = e !== b;
let r8e1_3 = e !== c;
let r8e1_4 = e !== d;

function main(): void {}
