// @target: es2015
enum E { a, b, c }

/*pruned*/;                       
let b: number = null as unknown as (number);

// operator <
/*pruned*/;     
/*pruned*/;     
let ra3 = E.a < b;
let ra4 = b < E.a;
let ra5 = E.a < 0;
let ra6 = 0 < E.a;

// operator >
/*pruned*/;     
/*pruned*/;     
let rb3 = E.a > b;
let rb4 = b > E.a;
let rb5 = E.a > 0;
let rb6 = 0 > E.a;

// operator <=
/*pruned*/;      
/*pruned*/;      
let rc3 = E.a <= b;
let rc4 = b <= E.a;
let rc5 = E.a <= 0;
let rc6 = 0 <= E.a;

// operator >=
/*pruned*/;      
/*pruned*/;      
let rd3 = E.a >= b;
let rd4 = b >= E.a;
let rd5 = E.a >= 0;
let rd6 = 0 >= E.a;

// operator ==
/*pruned*/;      
/*pruned*/;      
let re3 = E.a == b;
let re4 = b == E.a;
let re5 = E.a == 0;
let re6 = 0 == E.a;

// operator !=
/*pruned*/;      
/*pruned*/;      
let rf3 = E.a != b;
let rf4 = b != E.a;
let rf5 = E.a != 0;
let rf6 = 0 != E.a;

// operator ===
/*pruned*/;       
/*pruned*/;       
let rg3 = E.a === b;
let rg4 = b === E.a;
let rg5 = E.a === 0;
let rg6 = 0 === E.a;

// operator !==
/*pruned*/;       
/*pruned*/;       
let rh3 = E.a !== b;
let rh4 = b !== E.a;
let rh5 = E.a !== 0;
let rh6 = 0 !== E.a;

function main(): void {}
