// @target: es2015
// If one operand is the undefined or undefined value, it is treated as having the type of the
// other operand.

enum E {
    a,
    b
}

/*pruned*/;                           
let b: number = null as unknown as (number);

// operator *
/*pruned*/;             
let ra2 = undefined * b;
let ra3 = undefined * 1;
let ra4 = undefined * E.a;
/*pruned*/;             
let ra6 = b * undefined;
let ra7 = 0 * undefined;
let ra8 = E.b * undefined;

// operator /
/*pruned*/;             
let rb2 = undefined / b;
let rb3 = undefined / 1;
let rb4 = undefined / E.a;
/*pruned*/;             
let rb6 = b / undefined;
let rb7 = 0 / undefined;
let rb8 = E.b / undefined;

// operator %
/*pruned*/;             
let rc2 = undefined % b;
let rc3 = undefined % 1;
let rc4 = undefined % E.a;
/*pruned*/;             
let rc6 = b % undefined;
let rc7 = 0 % undefined;
let rc8 = E.b % undefined;

// operator -
/*pruned*/;             
let rd2 = undefined - b;
let rd3 = undefined - 1;
let rd4 = undefined - E.a;
/*pruned*/;             
let rd6 = b - undefined;
let rd7 = 0 - undefined;
let rd8 = E.b - undefined;

// operator <<
/*pruned*/;              
let re2 = undefined << b;
let re3 = undefined << 1;
let re4 = undefined << E.a;
/*pruned*/;              
let re6 = b << undefined;
let re7 = 0 << undefined;
let re8 = E.b << undefined;

// operator >>
/*pruned*/;              
let rf2 = undefined >> b;
let rf3 = undefined >> 1;
let rf4 = undefined >> E.a;
/*pruned*/;              
let rf6 = b >> undefined;
let rf7 = 0 >> undefined;
let rf8 = E.b >> undefined;

// operator >>>
/*pruned*/;               
let rg2 = undefined >>> b;
let rg3 = undefined >>> 1;
let rg4 = undefined >>> E.a;
/*pruned*/;               
let rg6 = b >>> undefined;
let rg7 = 0 >>> undefined;
let rg8 = E.b >>> undefined;

// operator &
/*pruned*/;             
let rh2 = undefined & b;
let rh3 = undefined & 1;
let rh4 = undefined & E.a;
/*pruned*/;             
let rh6 = b & undefined;
let rh7 = 0 & undefined;
let rh8 = E.b & undefined;

// operator ^
/*pruned*/;             
let ri2 = undefined ^ b;
let ri3 = undefined ^ 1;
let ri4 = undefined ^ E.a;
/*pruned*/;             
let ri6 = b ^ undefined;
let ri7 = 0 ^ undefined;
let ri8 = E.b ^ undefined;

// operator |
/*pruned*/;             
let rj2 = undefined | b;
let rj3 = undefined | 1;
let rj4 = undefined | E.a;
/*pruned*/;             
let rj6 = b | undefined;
let rj7 = 0 | undefined;
let rj8 = E.b | undefined;

function main(): void {}
