// @target: es2015
let [...a, x] = [1, 2, 3];  // Error, rest must be last element
[...a, x] = [1, 2, 3];      // Error, rest must be last element


function main(): void {}
