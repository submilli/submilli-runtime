// @target: es5, es2015
// @strict: true

let foo: string = "";

function f1 (bar: string = foo): string { // unexpected compiler error; works at runtime
    let foo: number = 2;
    return bar; // returns 1
}

function f2 (bar: (baz?: string) => string = (baz: string = foo) => baz): string { // unexpected compiler error; works at runtime
    let foo: number = 2;
    return bar(); // returns 1
}

function f3 (bar: number = foo, foo: number = 2): number { // correct compiler error, error at runtime
    return bar;
}

/*pruned*/;                        
              
 

/*pruned*/;               
            
 

/*pruned*/;                       
                
 

/*pruned*/;                             
                        
 

class Foo {
    constructor(public x: number = 12, public y: number = x) {}
}

function f8(foo1: string, bar: string = foo1): void { }


function main(): void {}
