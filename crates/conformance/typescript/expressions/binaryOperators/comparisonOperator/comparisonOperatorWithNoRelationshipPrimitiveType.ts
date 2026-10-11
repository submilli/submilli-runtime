// @target: es2015
enum E { a, b, c }

let a: number = null as unknown as (number);
let b: boolean = null as unknown as (boolean);
let c: string = null as unknown as (string);
let d: void = null as unknown as (void);
/*pruned*/;                       

// operator <
let r1a1 = a < b;
let r1a1_2 = a < c;
let r1a1_3 = a < d;
/*pruned*/;         // no error, expected

let r1b1 = b < a;
let r1b1_2 = b < c;
let r1b1_3 = b < d;
/*pruned*/;        

let r1c1 = c < a;
let r1c1_2 = c < b;
let r1c1_3 = c < d;
/*pruned*/;        

let r1d1 = d < a;
let r1d1_2 = d < b;
let r1d1_3 = d < c;
/*pruned*/;        

/*pruned*/;       // no error, expected
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        

// operator >
let r2a1 = a > b;
let r2a1_2 = a > c;
let r2a1_3 = a > d;
/*pruned*/;         // no error, expected

let r2b1 = b > a;
let r2b1_2 = b > c;
let r2b1_3 = b > d;
/*pruned*/;        

let r2c1 = c > a;
let r2c1_2 = c > b;
let r2c1_3 = c > d;
/*pruned*/;        

let r2d1 = d > a;
let r2d1_2 = d > b;
let r2d1_3 = d > c;
/*pruned*/;        

/*pruned*/;       // no error, expected
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        

// operator <=
let r3a1 = a <= b;
let r3a1_2 = a <= c;
let r3a1_3 = a <= d;
/*pruned*/;          // no error, expected

let r3b1 = b <= a;
let r3b1_2 = b <= c;
let r3b1_3 = b <= d;
/*pruned*/;         

let r3c1 = c <= a;
let r3c1_2 = c <= b;
let r3c1_3 = c <= d;
/*pruned*/;         

let r3d1 = d <= a;
let r3d1_2 = d <= b;
let r3d1_3 = d <= c;
/*pruned*/;         

/*pruned*/;        // no error, expected
/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

// operator >=
let r4a1 = a >= b;
let r4a1_2 = a >= c;
let r4a1_3 = a >= d;
/*pruned*/;          // no error, expected

let r4b1 = b >= a;
let r4b1_2 = b >= c;
let r4b1_3 = b >= d;
/*pruned*/;         

let r4c1 = c >= a;
let r4c1_2 = c >= b;
let r4c1_3 = c >= d;
/*pruned*/;         

let r4d1 = d >= a;
let r4d1_2 = d >= b;
let r4d1_3 = d >= c;
/*pruned*/;         

/*pruned*/;        // no error, expected
/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

// operator ==
let r5a1 = a == b;
let r5a1_2 = a == c;
let r5a1_3 = a == d;
/*pruned*/;          // no error, expected

let r5b1 = b == a;
let r5b1_2 = b == c;
let r5b1_3 = b == d;
/*pruned*/;         

let r5c1 = c == a;
let r5c1_2 = c == b;
let r5c1_3 = c == d;
/*pruned*/;         

let r5d1 = d == a;
let r5d1_2 = d == b;
let r5d1_3 = d == c;
/*pruned*/;         

/*pruned*/;        // no error, expected
/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

// operator !=
let r6a1 = a != b;
let r6a1_2 = a != c;
let r6a1_3 = a != d;
/*pruned*/;          // no error, expected

let r6b1 = b != a;
let r6b1_2 = b != c;
let r6b1_3 = b != d;
/*pruned*/;         

let r6c1 = c != a;
let r6c1_2 = c != b;
let r6c1_3 = c != d;
/*pruned*/;         

let r6d1 = d != a;
let r6d1_2 = d != b;
let r6d1_3 = d != c;
/*pruned*/;         

/*pruned*/;        // no error, expected
/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

// operator ===
let r7a1 = a === b;
let r7a1_2 = a === c;
let r7a1_3 = a === d;
/*pruned*/;           // no error, expected

let r7b1 = b === a;
let r7b1_2 = b === c;
let r7b1_3 = b === d;
/*pruned*/;          

let r7c1 = c === a;
let r7c1_2 = c === b;
let r7c1_3 = c === d;
/*pruned*/;          

let r7d1 = d === a;
let r7d1_2 = d === b;
let r7d1_3 = d === c;
/*pruned*/;          

/*pruned*/;         // no error, expected
/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

// operator !==
let r8a1 = a !== b;
let r8a1_2 = a !== c;
let r8a1_3 = a !== d;
/*pruned*/;           // no error, expected

let r8b1 = b !== a;
let r8b1_2 = b !== c;
let r8b1_3 = b !== d;
/*pruned*/;          

let r8c1 = c !== a;
let r8c1_2 = c !== b;
let r8c1_3 = c !== d;
/*pruned*/;          

let r8d1 = d !== a;
let r8d1_2 = d !== b;
let r8d1_3 = d !== c;
/*pruned*/;          

/*pruned*/;         // no error, expected
/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

function main(): void {}
