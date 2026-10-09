// @target: es2015
// @noImplicitAny: false

// these are widened to any at the point of assignment

let x1 = null;
let y1 = undefined;
let z1 = void 0;

// these are not widened

let x2: null = null as unknown as (null);
let y2: undefined = null as unknown as (undefined);

let x3: null = null;
let y3: undefined = undefined;
let z3: undefined = void 0;

// widen only when all constituents of union are widening

let x4 = null || null;
let y4 = undefined || undefined;
let z4 = void 0 || void 0;

let x5 = null || x2;
let y5 = undefined || y2;
let z5 = void 0 || y2;

function main(): void {}
