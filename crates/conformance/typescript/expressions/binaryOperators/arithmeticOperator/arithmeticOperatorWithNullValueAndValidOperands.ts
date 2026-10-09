// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the
// other operand.

enum E {
    a,
    b
}

/*pruned*/;                           
let b: number = null as unknown as (number);

// operator *
/*pruned*/;        
let ra2 = null * b;
let ra3 = null * 1;
let ra4 = null * E.a;
/*pruned*/;        
let ra6 = b * null;
let ra7 = 0 * null;
let ra8 = E.b * null;

// operator /
/*pruned*/;        
let rb2 = null / b;
let rb3 = null / 1;
let rb4 = null / E.a;
/*pruned*/;        
let rb6 = b / null;
let rb7 = 0 / null;
let rb8 = E.b / null;

// operator %
/*pruned*/;        
let rc2 = null % b;
let rc3 = null % 1;
let rc4 = null % E.a;
/*pruned*/;        
let rc6 = b % null;
let rc7 = 0 % null;
let rc8 = E.b % null;

// operator -
/*pruned*/;        
let rd2 = null - b;
let rd3 = null - 1;
let rd4 = null - E.a;
/*pruned*/;        
let rd6 = b - null;
let rd7 = 0 - null;
let rd8 = E.b - null;

// operator <<
/*pruned*/;         
let re2 = null << b;
let re3 = null << 1;
let re4 = null << E.a;
/*pruned*/;         
let re6 = b << null;
let re7 = 0 << null;
let re8 = E.b << null;

// operator >>
/*pruned*/;         
let rf2 = null >> b;
let rf3 = null >> 1;
let rf4 = null >> E.a;
/*pruned*/;         
let rf6 = b >> null;
let rf7 = 0 >> null;
let rf8 = E.b >> null;

// operator >>>
/*pruned*/;          
let rg2 = null >>> b;
let rg3 = null >>> 1;
let rg4 = null >>> E.a;
/*pruned*/;          
let rg6 = b >>> null;
let rg7 = 0 >>> null;
let rg8 = E.b >>> null;

// operator &
/*pruned*/;        
let rh2 = null & b;
let rh3 = null & 1;
let rh4 = null & E.a;
/*pruned*/;        
let rh6 = b & null;
let rh7 = 0 & null;
let rh8 = E.b & null;

// operator ^
/*pruned*/;        
let ri2 = null ^ b;
let ri3 = null ^ 1;
let ri4 = null ^ E.a;
/*pruned*/;        
let ri6 = b ^ null;
let ri7 = 0 ^ null;
let ri8 = E.b ^ null;

// operator |
/*pruned*/;        
let rj2 = null | b;
let rj3 = null | 1;
let rj4 = null | E.a;
/*pruned*/;        
let rj6 = b | null;
let rj7 = 0 | null;
let rj8 = E.b | null;

function main(): void {}
