// @target: es2015
enum E { a, b, c }

let a: boolean = null as unknown as (boolean);
let b: number = null as unknown as (number);
let c: string = null as unknown as (string);
let d: void = null as unknown as (void);
/*pruned*/;                       
let f: { a: string } = null as unknown as ({ a: string });
/*pruned*/;                               

function foo<T, U>(t: T, u: U): void {
    let r1 = t < u;
    let r2 = t > u;
    let r3 = t <= u;
    let r4 = t >= u;
    let r5 = t == u;
    let r6 = t != u;
    let r7 = t === u;
    let r8 = t !== u;

    // operator <
    let r1a1 = t < a;
    let r1a2 = t < b;
    let r1a3 = t < c;
    let r1a4 = t < d;
    /*pruned*/;      
    let r1a6 = t < f;
    /*pruned*/;      

    let r1b1 = a < t;
    let r1b2 = b < t;
    let r1b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r1b6 = f < t;
    /*pruned*/;      

    // operator >
    let r2a1 = t < a;
    let r2a2 = t < b;
    let r2a3 = t < c;
    let r2a4 = t < d;
    /*pruned*/;      
    let r2a6 = t < f;
    /*pruned*/;      

    let r2b1 = a < t;
    let r2b2 = b < t;
    let r2b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r2b6 = f < t;
    /*pruned*/;      

    // operator <=
    let r3a1 = t < a;
    let r3a2 = t < b;
    let r3a3 = t < c;
    let r3a4 = t < d;
    /*pruned*/;      
    let r3a6 = t < f;
    /*pruned*/;      

    let r3b1 = a < t;
    let r3b2 = b < t;
    let r3b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r3b6 = f < t;
    /*pruned*/;      

    // operator >=
    let r4a1 = t < a;
    let r4a2 = t < b;
    let r4a3 = t < c;
    let r4a4 = t < d;
    /*pruned*/;      
    let r4a6 = t < f;
    /*pruned*/;      

    let r4b1 = a < t;
    let r4b2 = b < t;
    let r4b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r4b6 = f < t;
    /*pruned*/;      

    // operator ==
    let r5a1 = t < a;
    let r5a2 = t < b;
    let r5a3 = t < c;
    let r5a4 = t < d;
    /*pruned*/;      
    let r5a6 = t < f;
    /*pruned*/;      

    let r5b1 = a < t;
    let r5b2 = b < t;
    let r5b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r5b6 = f < t;
    /*pruned*/;      

    // operator !=
    let r6a1 = t < a;
    let r6a2 = t < b;
    let r6a3 = t < c;
    let r6a4 = t < d;
    /*pruned*/;      
    let r6a6 = t < f;
    /*pruned*/;      

    let r6b1 = a < t;
    let r6b2 = b < t;
    let r6b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r6b6 = f < t;
    /*pruned*/;      

    // operator ===
    let r7a1 = t < a;
    let r7a2 = t < b;
    let r7a3 = t < c;
    let r7a4 = t < d;
    /*pruned*/;      
    let r7a6 = t < f;
    /*pruned*/;      

    let r7b1 = a < t;
    let r7b2 = b < t;
    let r7b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r7b6 = f < t;
    /*pruned*/;      

    // operator !==
    let r8a1 = t < a;
    let r8a2 = t < b;
    let r8a3 = t < c;
    let r8a4 = t < d;
    /*pruned*/;      
    let r8a6 = t < f;
    /*pruned*/;      

    let r8b1 = a < t;
    let r8b2 = b < t;
    let r8b3 = c < t;
    /*pruned*/;      
    /*pruned*/;      
    let r8b6 = f < t;
    /*pruned*/;      
}

function main(): void {}
