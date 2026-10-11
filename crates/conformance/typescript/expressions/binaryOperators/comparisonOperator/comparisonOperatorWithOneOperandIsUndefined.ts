// @target: es2015
let x: typeof undefined = null as unknown as (typeof undefined);

enum E { a, b, c }

function foo<T>(t: T): void {
    let foo_r1 = t < x;
    let foo_r2 = t > x;
    let foo_r3 = t <= x;
    let foo_r4 = t >= x;
    let foo_r5 = t == x;
    let foo_r6 = t != x;
    let foo_r7 = t === x;
    let foo_r8 = t !== x;

    let foo_r1_2 = x < t;
    let foo_r2_2 = x > t;
    let foo_r3_2 = x <= t;
    let foo_r4_2 = x >= t;
    let foo_r5_2 = x == t;
    let foo_r6_2 = x != t;
    let foo_r7_2 = x === t;
    let foo_r8_2 = x !== t;
}

let a: boolean = null as unknown as (boolean);
let b: number = null as unknown as (number);
let c: string = null as unknown as (string);
let d: void = null as unknown as (void);
/*pruned*/;                       
let f: {} = null as unknown as ({});
let g: string[] = null as unknown as (string[]);

// operator <
let r1a1 = x < a;
let r1a2 = x < b;
let r1a3 = x < c;
let r1a4 = x < d;
/*pruned*/;      
let r1a6 = x < f;
let r1a7 = x < g;

let r1b1 = a < x;
let r1b2 = b < x;
let r1b3 = c < x;
let r1b4 = d < x;
/*pruned*/;      
let r1b6 = f < x;
let r1b7 = g < x;

// operator >
let r2a1 = x > a;
let r2a2 = x > b;
let r2a3 = x > c;
let r2a4 = x > d;
/*pruned*/;      
let r2a6 = x > f;
let r2a7 = x > g;

let r2b1 = a > x;
let r2b2 = b > x;
let r2b3 = c > x;
let r2b4 = d > x;
/*pruned*/;      
let r2b6 = f > x;
let r2b7 = g > x;

// operator <=
let r3a1 = x <= a;
let r3a2 = x <= b;
let r3a3 = x <= c;
let r3a4 = x <= d;
/*pruned*/;       
let r3a6 = x <= f;
let r3a7 = x <= g;

let r3b1 = a <= x;
let r3b2 = b <= x;
let r3b3 = c <= x;
let r3b4 = d <= x;
/*pruned*/;       
let r3b6 = f <= x;
let r3b7 = g <= x;

// operator >=
let r4a1 = x >= a;
let r4a2 = x >= b;
let r4a3 = x >= c;
let r4a4 = x >= d;
/*pruned*/;       
let r4a6 = x >= f;
let r4a7 = x >= g;

let r4b1 = a >= x;
let r4b2 = b >= x;
let r4b3 = c >= x;
let r4b4 = d >= x;
/*pruned*/;       
let r4b6 = f >= x;
let r4b7 = g >= x;

// operator ==
let r5a1 = x == a;
let r5a2 = x == b;
let r5a3 = x == c;
let r5a4 = x == d;
/*pruned*/;       
let r5a6 = x == f;
let r5a7 = x == g;

let r5b1 = a == x;
let r5b2 = b == x;
let r5b3 = c == x;
let r5b4 = d == x;
/*pruned*/;       
let r5b6 = f == x;
let r5b7 = g == x;

// operator !=
let r6a1 = x != a;
let r6a2 = x != b;
let r6a3 = x != c;
let r6a4 = x != d;
/*pruned*/;       
let r6a6 = x != f;
let r6a7 = x != g;

let r6b1 = a != x;
let r6b2 = b != x;
let r6b3 = c != x;
let r6b4 = d != x;
/*pruned*/;       
let r6b6 = f != x;
let r6b7 = g != x;

// operator ===
let r7a1 = x === a;
let r7a2 = x === b;
let r7a3 = x === c;
let r7a4 = x === d;
/*pruned*/;        
let r7a6 = x === f;
let r7a7 = x === g;

let r7b1 = a === x;
let r7b2 = b === x;
let r7b3 = c === x;
let r7b4 = d === x;
/*pruned*/;        
let r7b6 = f === x;
let r7b7 = g === x;

// operator !==
let r8a1 = x !== a;
let r8a2 = x !== b;
let r8a3 = x !== c;
let r8a4 = x !== d;
/*pruned*/;        
let r8a6 = x !== f;
let r8a7 = x !== g;

let r8b1 = a !== x;
let r8b2 = b !== x;
let r8b3 = c !== x;
let r8b4 = d !== x;
/*pruned*/;        
let r8b6 = f !== x;
let r8b7 = g !== x;

function main(): void {}
