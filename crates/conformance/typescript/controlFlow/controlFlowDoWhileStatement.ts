// @target: es2015
let cond: boolean = null as unknown as (boolean);
function a(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    do {
        x; // string
    } while (cond)
}
function b(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    do {
        x; // string
        x = 42;
        break;
    } while (cond)
}
function c(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    do {
        x; // string
        x = undefined;
        if (typeof x === "string") continue;
        break;
    } while (cond)
}
function d(): void {
    let x: string | number = null as unknown as (string | number);
    x = 1000;
    do {
        x; // number
        x = "";
    } while (x = x.length)
    x; // number
}
function e(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    do {
        x = 42;
    } while (cond)
    x; // number
}
function f(): void {
    /*pruned*/;                                                                                                               
    /**/;  
    do {
        if (cond) {
            /**/;  
            break;
        }
        if (cond) {
            /**/;    
            continue;
        }
        /**/;   
    } while (cond)
    ;  // number | boolean | RegExp
}
function g(): void {
    /*pruned*/;                                                                                                               
    /**/;  
    do {
        if (cond) {
            /**/;  
            break;
        }
        if (cond) {
            /**/;    
            continue;
        }
        /**/;   
    } while (true)
    ;  // number
}


function main(): void {}
