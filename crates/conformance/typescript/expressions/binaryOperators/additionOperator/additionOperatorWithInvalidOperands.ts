// @target: es2015
// @strict: true
function foo(): void { }
class C {
    public a: string;
    static foo(): void { }
}
enum E { a, b, c }
/*pruned*/;                 

let a: boolean = null as unknown as (boolean);
let b: number = null as unknown as (number);
/*pruned*/;                                 
/*pruned*/;                                 

// boolean + every type except any and string
let r1 = a + a;
let r2 = a + b;
/*pruned*/;    

// number + every type except any and string
let r4 = b + a;
let r5 = b + b; // number + number is valid
/*pruned*/;    

// object + every type except any and string
/*pruned*/;    
/*pruned*/;    
/*pruned*/;    

// other cases
let r10 = a + true;
let r11 = true + false;
let r12 = true + 123;
let r13 = {} + {};
/*pruned*/;     
let r15 = b + foo;
let r16 = b + foo();
let r17 = b + C;
let r18 = E.a + new C();
let r19 = E.a + C.foo();
/*pruned*/;       

function main(): void {}
