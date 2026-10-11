// @target: es2015
// If one operand is the undefined or undefined value, it is treated as having the type of the
// other operand.

let a: boolean = null as unknown as (boolean);
let b: string = null as unknown as (string);
/*pruned*/;                                 

// operator *
let r1a1 = undefined * a;
let r1a2 = undefined * b;
/*pruned*/;              

let r1b1 = a * undefined;
let r1b2 = b * undefined;
/*pruned*/;              

let r1c1 = undefined * true;
let r1c2 = undefined * '';
let r1c3 = undefined * {};

let r1d1 = true * undefined;
let r1d2 = '' * undefined;
let r1d3 = {} * undefined;

// operator /
let r2a1 = undefined / a;
let r2a2 = undefined / b;
/*pruned*/;              

let r2b1 = a / undefined;
let r2b2 = b / undefined;
/*pruned*/;              

let r2c1 = undefined / true;
let r2c2 = undefined / '';
let r2c3 = undefined / {};

let r2d1 = true / undefined;
let r2d2 = '' / undefined;
let r2d3 = {} / undefined;

// operator %
let r3a1 = undefined % a;
let r3a2 = undefined % b;
/*pruned*/;              

let r3b1 = a % undefined;
let r3b2 = b % undefined;
/*pruned*/;              

let r3c1 = undefined % true;
let r3c2 = undefined % '';
let r3c3 = undefined % {};

let r3d1 = true % undefined;
let r3d2 = '' % undefined;
let r3d3 = {} % undefined;

// operator -
let r4a1 = undefined - a;
let r4a2 = undefined - b;
/*pruned*/;              

let r4b1 = a - undefined;
let r4b2 = b - undefined;
/*pruned*/;              

let r4c1 = undefined - true;
let r4c2 = undefined - '';
let r4c3 = undefined - {};

let r4d1 = true - undefined;
let r4d2 = '' - undefined;
let r4d3 = {} - undefined;

// operator <<
let r5a1 = undefined << a;
let r5a2 = undefined << b;
/*pruned*/;               

let r5b1 = a << undefined;
let r5b2 = b << undefined;
/*pruned*/;               

let r5c1 = undefined << true;
let r5c2 = undefined << '';
let r5c3 = undefined << {};

let r5d1 = true << undefined;
let r5d2 = '' << undefined;
let r5d3 = {} << undefined;

// operator >>
let r6a1 = undefined >> a;
let r6a2 = undefined >> b;
/*pruned*/;               

let r6b1 = a >> undefined;
let r6b2 = b >> undefined;
/*pruned*/;               

let r6c1 = undefined >> true;
let r6c2 = undefined >> '';
let r6c3 = undefined >> {};

let r6d1 = true >> undefined;
let r6d2 = '' >> undefined;
let r6d3 = {} >> undefined;

// operator >>>
let r7a1 = undefined >>> a;
let r7a2 = undefined >>> b;
/*pruned*/;                

let r7b1 = a >>> undefined;
let r7b2 = b >>> undefined;
/*pruned*/;                

let r7c1 = undefined >>> true;
let r7c2 = undefined >>> '';
let r7c3 = undefined >>> {};

let r7d1 = true >>> undefined;
let r7d2 = '' >>> undefined;
let r7d3 = {} >>> undefined;

// operator &
let r8a1 = undefined & a;
let r8a2 = undefined & b;
/*pruned*/;              

let r8b1 = a & undefined;
let r8b2 = b & undefined;
/*pruned*/;              

let r8c1 = undefined & true;
let r8c2 = undefined & '';
let r8c3 = undefined & {};

let r8d1 = true & undefined;
let r8d2 = '' & undefined;
let r8d3 = {} & undefined;

// operator ^
let r9a1 = undefined ^ a;
let r9a2 = undefined ^ b;
/*pruned*/;              

let r9b1 = a ^ undefined;
let r9b2 = b ^ undefined;
/*pruned*/;              

let r9c1 = undefined ^ true;
let r9c2 = undefined ^ '';
let r9c3 = undefined ^ {};

let r9d1 = true ^ undefined;
let r9d2 = '' ^ undefined;
let r9d3 = {} ^ undefined;

// operator |
let r10a1 = undefined | a;
let r10a2 = undefined | b;
/*pruned*/;               

let r10b1 = a | undefined;
let r10b2 = b | undefined;
/*pruned*/;               

let r10c1 = undefined | true;
let r10c2 = undefined | '';
let r10c3 = undefined | {};

let r10d1 = true | undefined;
let r10d2 = '' | undefined;
let r10d3 = {} | undefined;

function main(): void {}
