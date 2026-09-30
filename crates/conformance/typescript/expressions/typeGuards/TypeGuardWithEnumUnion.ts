// @target: es2015
enum Color { R, G, B }

function f1(x: Color | string): void {
    if (typeof x === "number") {
        let y = x;
        /*pruned*/;                                 
    }
    else {
        let z = x;
        let z_2: string = null as unknown as (string);
    }
}

function f2(x: Color | string | string[]): void {
    if (typeof x === "object") {
        let y = x;
        let y_2: string[] = null as unknown as (string[]);
    }
    if (typeof x === "number") {
        let z = x;
        /*pruned*/;                                 
    }
    else {
        let w = x;
        let w_2: string | string[] = null as unknown as (string | string[]);
    }
    if (typeof x === "string") {
        let a = x;
        let a_2: string = null as unknown as (string);
    }
    else {
        let b = x;
        /*pruned*/;                                                       
    }
}


function main(): void {}
