// @target: es5,es6

// ES6 Spec - 10.1.1 Static Semantics: UTF16Encoding (cp)
//  2. If cp ≤ 65535, return cp.
// (10000 == 65536)
let x = `\u{10000}`;


function main(): void {}
