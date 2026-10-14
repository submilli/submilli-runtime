// @target: es2015
type A1 = true | false;
type A2 = false | true;

function f1(): void {
    let a: A1 = null as unknown as (A1);
    let a_2: A2 = null as unknown as (A2);
    let a_3: true | false = null as unknown as (true | false);
    let a_4: false | true = null as unknown as (false | true);
}

function f2(a: true | false, b: boolean): void {
    a = b;
    b = a;
}

function f3(a: true | false, b: true | false): void {
    let x = a || b;
    let x_2 = a && b;
    let x_3 = !a;
}

function f4(t: true, f: false): void {
    let x1 = t && f;
    let x2 = f && t;
    let x3 = t || f;
    let x4 = f || t;
    let x5 = !t;
    let x6 = !f;
}

/*pruned*/;                                                        
/*pruned*/;                                                           
/*pruned*/;                                                           

function f5(b: boolean): void {
    /*pruned*/;      
    /*pruned*/;       
    /*pruned*/;   
}

function assertNever(x: never): never {
    throw new Error("Unexpected value");
}

function f10(x: true | false): "true" | "false" {
    switch (x) {
        case true: return "true";
        case false: return "false";
    }
}

function f11(x: true | false): "true" | "false" {
    switch (x) {
        case true: return "true";
        case false: return "false";
    }
    return assertNever(x);
}

function f12(x: true | false): void {
    if (x) {
        x;
    }
    else {
        x;
    }
}

function f13(x: true | false): void {
    if (x === true) {
        x;
    }
    else {
        x;
    }
}

type Item =
    { kind: true, a: string } |
    { kind: false, b: string };

function f20(x: Item): string {
    switch (x.kind) {
        case true: return x.a;
        case false: return x.b;
    }
}

function f21(x: Item): string {
    switch (x.kind) {
        case true: return x.a;
        case false: return x.b;
    }
    return assertNever(x);
}

function main(): void {}
