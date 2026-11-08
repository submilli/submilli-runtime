// @target: es2015
// operands of an enum type are treated as having the primitive type Number.

enum E {
    a,
    b
}
enum F {
    c,
    d
}

/*pruned*/;                           
let b: number = null as unknown as (number);
let c: E | F = null as unknown as (E | F);

// operator *
/*pruned*/;     
let ra2 = c * b;
let ra3 = c * c;
/*pruned*/;     
let ra5 = b * c;
/*pruned*/;       
let ra7 = E.a * b;
let ra8 = E.a * E.b;
let ra9 = E.a * 1;
/*pruned*/;        
let ra11 = b * E.b;
let ra12 = 1 * E.b;

// operator /
/*pruned*/;     
let rb2 = c / b;
let rb3 = c / c;
/*pruned*/;     
let rb5 = b / c;
/*pruned*/;       
let rb7 = E.a / b;
let rb8 = E.a / E.b;
let rb9 = E.a / 1;
/*pruned*/;        
let rb11 = b / E.b;
let rb12 = 1 / E.b;

// operator %
/*pruned*/;     
let rc2 = c % b;
let rc3 = c % c;
/*pruned*/;     
let rc5 = b % c;
/*pruned*/;       
let rc7 = E.a % b;
let rc8 = E.a % E.b;
let rc9 = E.a % 1;
/*pruned*/;        
let rc11 = b % E.b;
let rc12 = 1 % E.b;

// operator -
/*pruned*/;     
let rd2 = c - b;
let rd3 = c - c;
/*pruned*/;     
let rd5 = b - c;
/*pruned*/;       
let rd7 = E.a - b;
let rd8 = E.a - E.b;
let rd9 = E.a - 1;
/*pruned*/;        
let rd11 = b - E.b;
let rd12 = 1 - E.b;

// operator <<
/*pruned*/;      
let re2 = c << b;
let re3 = c << c;
/*pruned*/;      
let re5 = b << c;
/*pruned*/;        
let re7 = E.a << b;
let re8 = E.a << E.b;
let re9 = E.a << 1;
/*pruned*/;         
let re11 = b << E.b;
let re12 = 1 << E.b;

// operator >>
/*pruned*/;      
let rf2 = c >> b;
let rf3 = c >> c;
/*pruned*/;      
let rf5 = b >> c;
/*pruned*/;        
let rf7 = E.a >> b;
let rf8 = E.a >> E.b;
let rf9 = E.a >> 1;
/*pruned*/;         
let rf11 = b >> E.b;
let rf12 = 1 >> E.b;

// operator >>>
/*pruned*/;       
let rg2 = c >>> b;
let rg3 = c >>> c;
/*pruned*/;       
let rg5 = b >>> c;
/*pruned*/;         
let rg7 = E.a >>> b;
let rg8 = E.a >>> E.b;
let rg9 = E.a >>> 1;
/*pruned*/;          
let rg11 = b >>> E.b;
let rg12 = 1 >>> E.b;

// operator &
/*pruned*/;     
let rh2 = c & b;
let rh3 = c & c;
/*pruned*/;     
let rh5 = b & c;
/*pruned*/;       
let rh7 = E.a & b;
let rh8 = E.a & E.b;
let rh9 = E.a & 1;
/*pruned*/;        
let rh11 = b & E.b;
let rh12 = 1 & E.b;

// operator ^
/*pruned*/;     
let ri2 = c ^ b;
let ri3 = c ^ c;
/*pruned*/;     
let ri5 = b ^ c;
/*pruned*/;       
let ri7 = E.a ^ b;
let ri8 = E.a ^ E.b;
let ri9 = E.a ^ 1;
/*pruned*/;        
let ri11 = b ^ E.b;
let ri12 = 1 ^ E.b;

// operator |
/*pruned*/;     
let rj2 = c | b;
let rj3 = c | c;
/*pruned*/;     
let rj5 = b | c;
/*pruned*/;       
let rj7 = E.a | b;
let rj8 = E.a | E.b;
let rj9 = E.a | 1;
/*pruned*/;        
let rj11 = b | E.b;
let rj12 = 1 | E.b;

function main(): void {}
