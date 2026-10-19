// @target: es2015
// @noImplicitAny: true

let cond: boolean = null as unknown as (boolean);

function len(s: string): number {
    return s.length;
}

function f1(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "";
    while (cond) {
        x = len(x);
        x;
    }
    x;
}

function f2(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "";
    while (cond) {
        x;
        x = len(x);
    }
    x;
}

/*pruned*/;                                                            
/*pruned*/;                                                            

function g1(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "";
    while (cond) {
        /*pruned*/;
        x;
    }
    x;
}

function g2(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "";
    while (cond) {
        x;
        /*pruned*/;
    }
    x;
}

function asNumber(x: string | number): number {
    return +x;
}

function h1(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "0";
    while (cond) {
        x = +x + 1;
        x;
    }
}

function h2(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "0";
    while (cond) {
        x = asNumber(x) + 1;
        x;
    }
}

function h3(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "0";
    while (cond) {
        let y = asNumber(x);
        x = y + 1;
        x;
    }
}

function h4(): void {
    let x: string | number | boolean = null as unknown as (string | number | boolean);
    x = "0";
    while (cond) {
        x;
        let y = asNumber(x);
        x = y + 1;
        x;
    }
}


function main(): void {}
