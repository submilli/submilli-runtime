// @target: es2015

let strOrNum: string | number = null as unknown as (string | number);
let numOrBool: number | boolean = null as unknown as (number | boolean);
let str: string = null as unknown as (string);
let num: number = null as unknown as (number);
let bool: boolean = null as unknown as (boolean);

switch (strOrNum) {
    // Identical
    case strOrNum:
        break;

    // Constituents
    case str:
    case num:
        break;

    // Overlap in constituents
    case numOrBool:
        break;

    // No relation
    case bool:
        break;
}

function main(): void {}
