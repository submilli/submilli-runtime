// @target: es2015
// operands of an enum type are treated as having the primitive type Number.

enum E {
    a,
    b
}

/*pruned*/;                           
let b: number = null as unknown as (number);
/*pruned*/;                       

// operator **
/*pruned*/;     
/*pruned*/;     
/*pruned*/;     
/*pruned*/;     
/*pruned*/;     
/*pruned*/;       
let r7 = E.a ** b;
let r8 = E.a ** E.b;
let r9 = E.a ** 1;
/*pruned*/;        
let r11 = b ** E.b;
let r12 = 1 ** E.b;

function main(): void {}
