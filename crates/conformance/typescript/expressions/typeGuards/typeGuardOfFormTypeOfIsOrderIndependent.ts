// @target: es2015
let strOrNum: string | number = null as unknown as (string | number);
let strOrBool: string | boolean = null as unknown as (string | boolean);
let strOrFunc: string | (() => void) = null as unknown as (string | (() => void));
let numOrBool: number | boolean = null as unknown as (number | boolean);
let str: string = null as unknown as (string);
let num: number = null as unknown as (number);
let bool: boolean = null as unknown as (boolean);
let func: () => void = null as unknown as (() => void);

if ("string" === typeof strOrNum) {
    str = strOrNum;
}
else {
    num = strOrNum;
}
if ("function" === typeof strOrFunc) {
    func = strOrFunc;
}
else {
    str = strOrFunc;
}
if ("number" === typeof numOrBool) {
    num = numOrBool;
}
else {
    bool = numOrBool;
}
if ("boolean" === typeof strOrBool) {
    bool = strOrBool;
}
else {
    str = strOrBool;
}


function main(): void {}
