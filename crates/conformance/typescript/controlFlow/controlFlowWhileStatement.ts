// @target: es2015
let cond: boolean = null as unknown as (boolean);
function a(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    while (cond) {
        x; // string
    }
}
function b(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    while (cond) {
        x; // string
        x = 42;
        break;
    }
}
function c(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    while (cond) {
        x; // string
        x = null;
        if (typeof x === "string") continue;
        break;
    }
}
function d(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    while (x = x.length) {
        x; // number
        x = "";
    }
}
function e(): void {
    let x: string | number = null as unknown as (string | number);
    x = "";
    while (cond) {
        x; // string | number
        x = 42;
        x; // number
    }
    x; // string | number
}
function f(): void {
    /*pruned*/;                                                                                                               
    /**/;  
    while (cond) {
        if (cond) {
            /**/;  
            break;
        }
        if (cond) {
            /**/;    
            continue;
        }
        /**/;   
    }
    ;  // string | number | boolean | RegExp
}
function g(): void {
    /*pruned*/;                                                                                                               
    /**/;  
    while (true) {
        if (cond) {
            /**/;  
            break;
        }
        if (cond) {
            /**/;    
            continue;
        }
        /**/;   
    }
    ;  // number
}
function h1(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "";
    while (x > 1) {
        x; // string | number
        x = 1;
        x; // number
    }
    x; // string | number
}
function len(s: string | number): number { return null as unknown as (number); }
function h2(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "";
    while (cond) {
        x = len(x);
        x; // number
    }
    x; // string | number
}
function h3(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "";
    while (cond) {
        x; // string | number
        x = len(x);
    }
    x; // string | number
}


function main(): void {}
