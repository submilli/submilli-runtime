// @target: es2015
enum E { a, b, c }

function foo<T>(t: T): void {
    let foo_r1 = t < null;
    let foo_r2 = t > null;
    let foo_r3 = t <= null;
    let foo_r4 = t >= null;
    let foo_r5 = t == null;
    let foo_r6 = t != null;
    let foo_r7 = t === null;
    let foo_r8 = t !== null;

    let foo_r1_2 = null < t;
    let foo_r2_2 = null > t;
    let foo_r3_2 = null <= t;
    let foo_r4_2 = null >= t;
    let foo_r5_2 = null == t;
    let foo_r6_2 = null != t;
    let foo_r7_2 = null === t;
    let foo_r8_2 = null !== t;
}

let a: boolean = null as unknown as (boolean);
let b: number = null as unknown as (number);
let c: string = null as unknown as (string);
let d: void = null as unknown as (void);
/*pruned*/;                       
let f: {} = null as unknown as ({});
let g: string[] = null as unknown as (string[]);

// operator <
let r1a1 = null < a;
let r1a2 = null < b;
let r1a3 = null < c;
let r1a4 = null < d;
/*pruned*/;         
let r1a6 = null < f;
let r1a7 = null < g;

let r1b1 = a < null;
let r1b2 = b < null;
let r1b3 = c < null;
let r1b4 = d < null;
/*pruned*/;         
let r1b6 = f < null;
let r1b7 = g < null;

// operator >
let r2a1 = null > a;
let r2a2 = null > b;
let r2a3 = null > c;
let r2a4 = null > d;
/*pruned*/;         
let r2a6 = null > f;
let r2a7 = null > g;

let r2b1 = a > null;
let r2b2 = b > null;
let r2b3 = c > null;
let r2b4 = d > null;
/*pruned*/;         
let r2b6 = f > null;
let r2b7 = g > null;

// operator <=
let r3a1 = null <= a;
let r3a2 = null <= b;
let r3a3 = null <= c;
let r3a4 = null <= d;
/*pruned*/;          
let r3a6 = null <= f;
let r3a7 = null <= g;

let r3b1 = a <= null;
let r3b2 = b <= null;
let r3b3 = c <= null;
let r3b4 = d <= null;
/*pruned*/;          
let r3b6 = f <= null;
let r3b7 = g <= null;

// operator >=
let r4a1 = null >= a;
let r4a2 = null >= b;
let r4a3 = null >= c;
let r4a4 = null >= d;
/*pruned*/;          
let r4a6 = null >= f;
let r4a7 = null >= g;

let r4b1 = a >= null;
let r4b2 = b >= null;
let r4b3 = c >= null;
let r4b4 = d >= null;
/*pruned*/;          
let r4b6 = f >= null;
let r4b7 = g >= null;

// operator ==
let r5a1 = null == a;
let r5a2 = null == b;
let r5a3 = null == c;
let r5a4 = null == d;
/*pruned*/;          
let r5a6 = null == f;
let r5a7 = null == g;

let r5b1 = a == null;
let r5b2 = b == null;
let r5b3 = c == null;
let r5b4 = d == null;
/*pruned*/;          
let r5b6 = f == null;
let r5b7 = g == null;

// operator !=
let r6a1 = null != a;
let r6a2 = null != b;
let r6a3 = null != c;
let r6a4 = null != d;
/*pruned*/;          
let r6a6 = null != f;
let r6a7 = null != g;

let r6b1 = a != null;
let r6b2 = b != null;
let r6b3 = c != null;
let r6b4 = d != null;
/*pruned*/;          
let r6b6 = f != null;
let r6b7 = g != null;

// operator ===
let r7a1 = null === a;
let r7a2 = null === b;
let r7a3 = null === c;
let r7a4 = null === d;
/*pruned*/;           
let r7a6 = null === f;
let r7a7 = null === g;

let r7b1 = a === null;
let r7b2 = b === null;
let r7b3 = c === null;
let r7b4 = d === null;
/*pruned*/;           
let r7b6 = f === null;
let r7b7 = g === null;

// operator !==
let r8a1 = null !== a;
let r8a2 = null !== b;
let r8a3 = null !== c;
let r8a4 = null !== d;
/*pruned*/;           
let r8a6 = null !== f;
let r8a7 = null !== g;

let r8b1 = a !== null;
let r8b2 = b !== null;
let r8b3 = c !== null;
let r8b4 = d !== null;
/*pruned*/;           
let r8b6 = f !== null;
let r8b7 = g !== null;

function main(): void {}
