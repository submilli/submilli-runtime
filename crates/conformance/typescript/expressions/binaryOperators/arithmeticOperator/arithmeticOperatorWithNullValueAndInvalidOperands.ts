// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the
// other operand.

let a: boolean = null as unknown as (boolean);
let b: string = null as unknown as (string);
/*pruned*/;                                 

// operator *
let r1a1 = null * a;
let r1a2 = null * b;
/*pruned*/;         

let r1b1 = a * null;
let r1b2 = b * null;
/*pruned*/;         

let r1c1 = null * true;
let r1c2 = null * '';
let r1c3 = null * {};

let r1d1 = true * null;
let r1d2 = '' * null;
let r1d3 = {} * null;

// operator /
let r2a1 = null / a;
let r2a2 = null / b;
/*pruned*/;         

let r2b1 = a / null;
let r2b2 = b / null;
/*pruned*/;         

let r2c1 = null / true;
let r2c2 = null / '';
let r2c3 = null / {};

let r2d1 = true / null;
let r2d2 = '' / null;
let r2d3 = {} / null;

// operator %
let r3a1 = null % a;
let r3a2 = null % b;
/*pruned*/;         

let r3b1 = a % null;
let r3b2 = b % null;
/*pruned*/;         

let r3c1 = null % true;
let r3c2 = null % '';
let r3c3 = null % {};

let r3d1 = true % null;
let r3d2 = '' % null;
let r3d3 = {} % null;

// operator -
let r4a1 = null - a;
let r4a2 = null - b;
/*pruned*/;         

let r4b1 = a - null;
let r4b2 = b - null;
/*pruned*/;         

let r4c1 = null - true;
let r4c2 = null - '';
let r4c3 = null - {};

let r4d1 = true - null;
let r4d2 = '' - null;
let r4d3 = {} - null;

// operator <<
let r5a1 = null << a;
let r5a2 = null << b;
/*pruned*/;          

let r5b1 = a << null;
let r5b2 = b << null;
/*pruned*/;          

let r5c1 = null << true;
let r5c2 = null << '';
let r5c3 = null << {};

let r5d1 = true << null;
let r5d2 = '' << null;
let r5d3 = {} << null;

// operator >>
let r6a1 = null >> a;
let r6a2 = null >> b;
/*pruned*/;          

let r6b1 = a >> null;
let r6b2 = b >> null;
/*pruned*/;          

let r6c1 = null >> true;
let r6c2 = null >> '';
let r6c3 = null >> {};

let r6d1 = true >> null;
let r6d2 = '' >> null;
let r6d3 = {} >> null;

// operator >>>
let r7a1 = null >>> a;
let r7a2 = null >>> b;
/*pruned*/;           

let r7b1 = a >>> null;
let r7b2 = b >>> null;
/*pruned*/;           

let r7c1 = null >>> true;
let r7c2 = null >>> '';
let r7c3 = null >>> {};

let r7d1 = true >>> null;
let r7d2 = '' >>> null;
let r7d3 = {} >>> null;

// operator &
let r8a1 = null & a;
let r8a2 = null & b;
/*pruned*/;         

let r8b1 = a & null;
let r8b2 = b & null;
/*pruned*/;         

let r8c1 = null & true;
let r8c2 = null & '';
let r8c3 = null & {};

let r8d1 = true & null;
let r8d2 = '' & null;
let r8d3 = {} & null;

// operator ^
let r9a1 = null ^ a;
let r9a2 = null ^ b;
/*pruned*/;         

let r9b1 = a ^ null;
let r9b2 = b ^ null;
/*pruned*/;         

let r9c1 = null ^ true;
let r9c2 = null ^ '';
let r9c3 = null ^ {};

let r9d1 = true ^ null;
let r9d2 = '' ^ null;
let r9d3 = {} ^ null;

// operator |
let r10a1 = null | a;
let r10a2 = null | b;
/*pruned*/;          

let r10b1 = a | null;
let r10b2 = b | null;
/*pruned*/;          

let r10c1 = null | true;
let r10c2 = null | '';
let r10c3 = null | {};

let r10d1 = true | null;
let r10d2 = '' | null;
let r10d3 = {} | null;

function main(): void {}
