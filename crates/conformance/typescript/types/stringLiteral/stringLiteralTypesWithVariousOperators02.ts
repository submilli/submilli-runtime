// @target: es2015
// @declaration: true

let abc: "ABC" = null as unknown as ("ABC");
let xyz: "XYZ" = null as unknown as ("XYZ");
let abcOrXyz: "ABC" | "XYZ" = null as unknown as ("ABC" | "XYZ");
let abcOrXyzOrNumber: "ABC" | "XYZ" | number = null as unknown as ("ABC" | "XYZ" | number);

let a = abcOrXyzOrNumber + 100;
let b = 100 + abcOrXyzOrNumber;
let c = abcOrXyzOrNumber + abcOrXyzOrNumber;
let d = abcOrXyzOrNumber + true;
let e = false + abcOrXyzOrNumber;
let f = abcOrXyzOrNumber++;
/*pruned*/;                
/*pruned*/;                   
/*pruned*/;                   
let j = abc < xyz;
let k = abc === xyz;
let l = abc != xyz;

function main(): void {}
