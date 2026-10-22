// A function expression or regular expression literal compared with `===` makes
// a new object, so the comparison can't be true (tsc: TS2839). An arrow compares
// like any other operand, as in tsc, and is typed by the other side only when
// that side is a function type. Object and array literals compare by value, so
// they are not reported.
// expect-error-count: 4
// expect-error: this comparison is always `false`: a function expression makes a new function
// expect-error: this comparison is always `true`: a regular expression literal makes a new object
const a = undefined === function (): number { return 1; };
const b = null === function (): number { return 1; };
const fn: () => number = () => 2;
const c = fn === function (): number { return 1; };
const r = /a/;
const d = /a/ !== r;

const e = undefined === (() => 1);
const f = (() => 1) === null;
const g = fn === ((): number => 1);
const h = (function (): number { return 1; }) === fn;
const i = [1, 2] === [1, 2];

function main(): void {}
