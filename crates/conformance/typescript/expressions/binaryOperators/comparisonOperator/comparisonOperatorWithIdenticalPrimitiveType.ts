// @target: es2015
enum E { a, b, c }

let a: number = null as unknown as (number);
let b: boolean = null as unknown as (boolean);
let c: string = null as unknown as (string);
let d: void = null as unknown as (void);
/*pruned*/;                       

// operator <
let ra1 = a < a;
let ra2 = b < b;
let ra3 = c < c;
/*pruned*/;     
/*pruned*/;     
let ra6 = null < null;
let ra7 = undefined < undefined;

// operator >
let rb1 = a > a;
let rb2 = b > b;
let rb3 = c > c;
/*pruned*/;     
/*pruned*/;     
let rb6 = null > null;
let rb7 = undefined > undefined;

// operator <=
let rc1 = a <= a;
let rc2 = b <= b;
let rc3 = c <= c;
/*pruned*/;      
/*pruned*/;      
let rc6 = null <= null;
let rc7 = undefined <= undefined;

// operator >=
let rd1 = a >= a;
let rd2 = b >= b;
let rd3 = c >= c;
/*pruned*/;      
/*pruned*/;      
let rd6 = null >= null;
let rd7 = undefined >= undefined;

// operator ==
let re1 = a == a;
let re2 = b == b;
let re3 = c == c;
let re4 = d == d;
/*pruned*/;      
let re6 = null == null;
let re7 = undefined == undefined;

// operator !=
let rf1 = a != a;
let rf2 = b != b;
let rf3 = c != c;
let rf4 = d != d;
/*pruned*/;      
let rf6 = null != null;
let rf7 = undefined != undefined;

// operator ===
let rg1 = a === a;
let rg2 = b === b;
let rg3 = c === c;
let rg4 = d === d;
/*pruned*/;       
let rg6 = null === null;
let rg7 = undefined === undefined;

// operator !==
let rh1 = a !== a;
let rh2 = b !== b;
let rh3 = c !== c;
let rh4 = d !== d;
/*pruned*/;       
let rh6 = null !== null;
let rh7 = undefined !== undefined;

function main(): void {}
