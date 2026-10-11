// @target: es2015
let unionNumberString: number | string = null as unknown as (number | string);
class C { }
class D extends C { foo1(): void { } }
class E extends C { foo2(): void { } }
/*pruned*/;                                     

let num: number = null as unknown as (number);
let str: string = null as unknown as (string);
/*pruned*/;                       
/*pruned*/;                       
/*pruned*/;                       

// A union type U is assignable to a type T if each type in U is assignable to T
/**/; 
/**/; 
/*pruned*/;  // ok
/**/; 
/**/; 
/*pruned*/;  // error e is not assignable to d
/**/; 
/**/; 
/*pruned*/;  // error d is not assignable to e
num = num;
num = str;
num = unionNumberString; // error string is not assignable to number
str = num;
str = str;
str = unionNumberString; // error since number is not assignable to string

// A type T is assignable to a union type U if T is assignable to any type in U
/**/; 
/**/; 
/*pruned*/;  // error since C is not assinable to either D or E
/**/; 
/**/; 
/*pruned*/;  // ok
/**/; 
/**/; 
/*pruned*/;  // ok
num = num;
str = num;
unionNumberString = num; // ok 
num = str;
str = str;
unionNumberString = str; // ok

// Any
/*pruned*/;                                
/*pruned*/;      
/*pruned*/;                
/*pruned*/;      
/*pruned*/;                

// null
/*pruned*/;    
unionNumberString = null;

// undefined
/*pruned*/;    
unionNumberString = undefined;

// type parameters
function foo<T, U>(t: T, u: U): void {
    t = u; // error
    u = t; // error
    /*pruned*/;                                
    /**/;  // ok
    /**/;  // ok
    /**/;    
    /**/;  // error U not assignable to T
    /**/;  // error T not assignable to U
}


function main(): void {}
