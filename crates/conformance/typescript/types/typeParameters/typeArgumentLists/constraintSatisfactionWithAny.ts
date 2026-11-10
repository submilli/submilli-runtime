// @target: es2015
// any is not a valid type argument unless there is no constraint, or the constraint is any

function foo<T extends String>(x: T): T { return null; }
function foo2<T extends { x: number }>(x: T): T { return null; }
//function foo3<T extends T[]>(x: T): T { return null; }
/*pruned*/;                                                         
let a;
foo(a);
foo2(a);
//foo3(a);
/**/;   

let b: number = null as unknown as (number);
/*pruned*/; 
/*pruned*/;  
//foo3<any>(b);
/*pruned*/;  

//function foo5<T extends String, U extends T>(x: T, y: U): T { return null; }
//foo5(a, a);
//foo5<any, any>(b, b);

class C<T extends String> {
    constructor(public x: T) { }
}

let c1 = new C(a);
/*pruned*/;            

class C2<T extends { x: number }> {
    constructor(public x: T) { }
}

let c3 = new C2(a);
/*pruned*/;             

//class C3<T extends T[]> {
//    constructor(public x: T) { }
//}

//var c5 = new C3(a);
//var c6 = new C3<any>(b);

/*pruned*/;                        
                                
 

/*pruned*/;        
/*pruned*/;             




function main(): void {}
