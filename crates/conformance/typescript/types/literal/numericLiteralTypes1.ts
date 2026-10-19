// @target: es2015
type A1 = 1;
type A2 = 1.0;
type A3 = 1e0;
type A4 = 10e-1;
type A5 = 1 | 1.0 | 1e0 | 10e-1;

function f1(): void {
    let a: A1 = 1;
    let a_2: A2 = 1;
    let a_3: A3 = 1;
    let a_4: A4 = 1;
    let a_5: A5 = 1;
}

type B1 = -1 | 0 | 1;
type B2 = 1 | 0 | -1;
type B3 = 0 | -1 | 1;

function f2(): void {
    let b: B1 = -1;
    let b_2: B2 = 0;
    let b_3: B3 = 1;
}

function f3(a: 1, b: 0 | 1 | 2): void {
    let x = a + b;
    let x_2 = a - b;
    let x_3 = a * b;
    let x_4 = a / b;
    let x_5 = a % b;
    let x_6 = a | b;
    let x_7 = a & b;
    let x_8 = a ^ b;
    let x_9 = -b;
    let x_10 = ~b;
    let y = a == b;
    let y_2 = a != b;
    let y_3 = a === b;
    let y_4 = a !== b;
    let y_5 = a > b;
    let y_6 = a < b;
    let y_7 = a >= b;
    let y_8 = a <= b;
    let y_9 = !b;
}

function f4(a: 1, b: 0 | 1 | 2): void {
    a++;
    b++;
}

/*pruned*/;                                                     
/*pruned*/;                                                       
/*pruned*/;                                                          

function f5(a: 1, b: 0 | 1 | 2): void {
    /*pruned*/;   
    /*pruned*/;   
    /*pruned*/;   
    /*pruned*/;   
    /*pruned*/;   
}

function assertNever(x: never): never {
    throw new Error("Unexpected value");
}

type Tag = 0 | 1 | 2;

function f10(x: Tag): "a" | "b" | "c" {
    switch (x) {
        case 0: return "a";
        case 1: return "b";
        case 2: return "c";
    }
}

function f11(x: Tag): "a" | "b" | "c" {
    switch (x) {
        case 0: return "a";
        case 1: return "b";
        case 2: return "c";
    }
    return assertNever(x);
}

function f12(x: Tag): void {
    if (x) {
        x;
    }
    else {
        x;
    }
}

function f13(x: Tag): void {
    if (x === 0 || x === 2) {
        x;
    }
    else {
        x;
    }
}

function f14(x: 0 | 1 | 2, y: string): void {
    let a = x && y;
    let b = x || y;
}

function f15(x: 0 | false, y: 1 | "one"): void {
    let a = x && y;
    let b = y && x;
    let c = x || y;
    let d = y || x;
    let e = !x;
    let f = !y;
}

type Item =
    { kind: 0, a: string } |
    { kind: 1, b: string } |
    { kind: 2, c: string };

function f20(x: Item): string {
    switch (x.kind) {
        case 0: return x.a;
        case 1: return x.b;
        case 2: return x.c;
    }
}

function f21(x: Item): string {
    switch (x.kind) {
        case 0: return x.a;
        case 1: return x.b;
        case 2: return x.c;
    }
    return assertNever(x);
}

function main(): void {}
