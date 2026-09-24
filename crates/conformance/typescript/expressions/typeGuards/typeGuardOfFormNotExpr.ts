// @target: es2015
let str: string = null as unknown as (string);
let bool: boolean = null as unknown as (boolean);
let num: number = null as unknown as (number);
let strOrNum: string | number = null as unknown as (string | number);
let strOrNumOrBool: string | number | boolean = null as unknown as (string | number | boolean);
let numOrBool: number | boolean = null as unknown as (number | boolean);

// A type guard of the form !expr
// - when true, narrows the type of x by expr when false, or
// - when false, narrows the type of x by expr when true.

// !typeguard1
if (!(typeof strOrNum === "string")) {
    num === strOrNum; // number
}
else {
    str = strOrNum; // string
}
// !(typeguard1 || typeguard2)
if (!(typeof strOrNumOrBool === "string" || typeof strOrNumOrBool === "number")) {
    bool = strOrNumOrBool; // boolean
}
else {
    strOrNum = strOrNumOrBool; // string | number
}
// !(typeguard1) || !(typeguard2)
if (!(typeof strOrNumOrBool !== "string") || !(typeof strOrNumOrBool !== "number")) {
    strOrNum = strOrNumOrBool; // string | number
}
else {
    bool = strOrNumOrBool; // boolean
}
// !(typeguard1 && typeguard2)
if (!(typeof strOrNumOrBool !== "string" && typeof strOrNumOrBool !== "number")) {
    strOrNum = strOrNumOrBool; // string | number
}
else {
    bool = strOrNumOrBool; // boolean
}
// !(typeguard1) && !(typeguard2)
if (!(typeof strOrNumOrBool === "string") && !(typeof strOrNumOrBool === "number")) {
    bool = strOrNumOrBool; // boolean
}
else {
    strOrNum = strOrNumOrBool; // string | number
}
// !(typeguard1) && simpleExpr
if (!(typeof strOrNumOrBool === "string") && numOrBool !== strOrNumOrBool) {
    numOrBool = strOrNumOrBool; // number | boolean
}
else {
    let r1: string | number | boolean = strOrNumOrBool; // string | number | boolean
}

function main(): void {}
