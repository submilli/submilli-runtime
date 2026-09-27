// @target: es2015
// @declaration: true

let abc: "ABC" = null as unknown as ("ABC");
let xyz: "XYZ" = null as unknown as ("XYZ");
let abcOrXyz: "ABC" | "XYZ" = null as unknown as ("ABC" | "XYZ");
let abcOrXyzOrNumber: "ABC" | "XYZ" | number = null as unknown as ("ABC" | "XYZ" | number);

let a = "" + abc;
let b = abc + "";
let c = 10 + abc;
let d = abc + 10;
let e = xyz + abc;
let f = abc + xyz;
let g = true + abc;
let h = abc + true;
let i = abc + abcOrXyz + xyz;
let j = abcOrXyz + abcOrXyz;
let k = +abcOrXyz;
let l = -abcOrXyz;
let m = abcOrXyzOrNumber + "";
let n = "" + abcOrXyzOrNumber;
let o = abcOrXyzOrNumber + abcOrXyz;
let p = abcOrXyz + abcOrXyzOrNumber;
let q = !abcOrXyzOrNumber;
/*pruned*/;               
let s = abcOrXyzOrNumber < abcOrXyzOrNumber;
let t = abcOrXyzOrNumber >= abcOrXyz;
let u = abc === abcOrXyz;
let v = abcOrXyz === abcOrXyzOrNumber;

function main(): void {}
