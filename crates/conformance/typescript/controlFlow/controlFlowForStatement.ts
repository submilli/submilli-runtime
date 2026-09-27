// @target: es2015
let cond: boolean = null as unknown as (boolean);
function a(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    for (x = ""; cond; x = 5) {
        x; // string | number
    }
}
function b(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    for (x = 5; cond; x = x.length) {
        x; // number
        x = "";
    }
}
function c(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    for (x = 5; x = x.toExponential(); x = 5) {
        x; // string
    }
}
function d(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    for (x = ""; typeof x === "string"; x = 5) {
        x; // string
    }
}
function e(): void {
    /*pruned*/;                                                                                         
    /*pruned*/;                                               
                              
     
}
function f(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    for (; typeof x !== "string";) {
        x; // number | boolean
        if (typeof x === "number") break;
        x = null;
    }
    x; // string | number
}


function main(): void {}
