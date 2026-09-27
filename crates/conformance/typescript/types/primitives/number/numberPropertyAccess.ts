// @target: es2015
let x = 1;
let a = x.toExponential();
let b = x.hasOwnProperty('toFixed');

let c = x['toExponential']();
let d = x['hasOwnProperty']('toFixed');

function main(): void {}
