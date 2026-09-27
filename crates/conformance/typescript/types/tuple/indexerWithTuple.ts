// @target: es2015
let strNumTuple: [string, number] = ["foo", 10];
let numTupleTuple: [number, [string, number]] = [10, ["bar", 20]];
let unionTuple1: [number, string| number] = [10, "foo"];
let unionTuple2: [boolean, string| number] = [true, "foo"];

// no error
let idx0 = 0;
let idx1 = 1;
let ele10 = strNumTuple[0]; // string
let ele11 = strNumTuple[1]; // number
let ele12 = strNumTuple[2]; // string | number
let ele13 = strNumTuple[idx0]; // string | number
let ele14 = strNumTuple[idx1]; // string | number
let ele15 = strNumTuple["0"]; // string
let ele16 = strNumTuple["1"]; // number
let strNumTuple1 = numTupleTuple[1];  //[string, number];
let ele17 = numTupleTuple[2]; // number | [string, number]
let ele19 = strNumTuple[-1]   // undefined

let eleUnion10 = unionTuple1[0]; // number
let eleUnion11 = unionTuple1[1]; // string | number
let eleUnion12 = unionTuple1[2]; // string | number
let eleUnion13 = unionTuple1[idx0]; // string | number
let eleUnion14 = unionTuple1[idx1]; // string | number
let eleUnion15 = unionTuple1["0"]; // number
let eleUnion16 = unionTuple1["1"]; // string | number

let eleUnion20 = unionTuple2[0]; // boolean
let eleUnion21 = unionTuple2[1]; // string | number
let eleUnion22 = unionTuple2[2]; // string | number | boolean
let eleUnion23 = unionTuple2[idx0]; // string | number | boolean
let eleUnion24 = unionTuple2[idx1]; // string | number | boolean
let eleUnion25 = unionTuple2["0"]; // boolean
let eleUnion26 = unionTuple2["1"]; // string | number

/*pruned*/;                     // string
/*pruned*/;                     // number
/*pruned*/;                     // undefined


function main(): void {}
