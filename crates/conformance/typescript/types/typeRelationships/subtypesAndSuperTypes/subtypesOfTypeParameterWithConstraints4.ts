// @target: es2015
// checking whether other types are subtypes of type parameters with constraints

class Foo { foo: number; }
function f<T extends Foo, U extends Foo, V>(t: T, u: U, v: V): void {
    // ok
    let r = true ? t : u;
    let r_2 = true ? u : t;

    // ok
    let r2 = true ? t : v;
    let r2_2 = true ? v : t;

    // ok
    let r3 = true ? v : u;
    let r3_2 = true ? u : v;

    // ok
    let r4 = true ? t : new Foo();
    let r4_2 = true ? new Foo() : t;

    // ok
    let r5 = true ? u : new Foo();
    let r5_2 = true ? new Foo() : u;

    // ok
    let r6 = true ? v : new Foo();
    let r6_2 = true ? new Foo() : v;

}

class B1<T> {
    foo: T;
}

/*pruned*/;                                                
                     
                 
 

/*pruned*/;                                                
                     
                 
 

/*pruned*/;                                                
                     
                    
 

/*pruned*/;                                              
                   
                 
 

/*pruned*/;                                              
                   
                    
 

/*pruned*/;                                              
                   
                    
 

/*pruned*/;                                              
                   
                    
 

/*pruned*/;                                              
                   
                 
 

/*pruned*/;                                              
                   
                    
 

function main(): void {}
