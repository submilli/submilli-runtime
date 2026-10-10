// @target: es2015
// @strict: true

function f1(obj: { a?: string }): void {
    if (obj.a) {
        obj = {};
        let a1 = obj["a"];  // string | undefined
        let a2 = obj.a;  // string | undefined
    }
}

function f2(obj: [number, string] | null[]): void {
    let a0 = obj[0];  // number | null
    let a1 = obj[1];  // string | null
    let [b0, b1] = obj;
    /*pruned*/;      
    if (obj[0] && obj[1]) {
        let c0 = obj[0];  // number
        let c1 = obj[1];  // string
        let [d0, d1] = obj;
        /*pruned*/;      
    }
}

function f3(obj: { a?: number, b?: string }): void {
    if (obj.a && obj.b) {
        let { a, b } = obj;  // number, string
        /*pruned*/;      
    }
}

function f4(): void {
    let x: boolean = null as unknown as (boolean);
    /*pruned*/;   // Error
    /*pruned*/;          // Error
    /*pruned*/;               // Errpr
}

// Repro from #31770

type KeyValue = [string, string?];
let [key, value]: KeyValue = ["foo"];
value.toUpperCase();  // Error


function main(): void {}
