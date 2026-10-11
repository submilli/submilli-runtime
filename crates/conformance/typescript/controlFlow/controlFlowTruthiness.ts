// @target: es2015
// @strictNullChecks: true

function foo(): string | undefined { return null as unknown as (string | undefined); }

function f1(): void {
    let x = foo();
    if (x) {
        x; // string
    }
    else {
        x; // string | undefined
    }
}

function f2(): void {
    let x: string | undefined = null as unknown as (string | undefined);
    x = foo();
    if (x) {
        x; // string
    }
    else {
        x; // string | undefined
    }
}

function f3(): void {
    let x: string | undefined = null as unknown as (string | undefined);
    if (x = foo()) {
        x; // string
    }
    else {
        x; // string | undefined
    }
}

function f4(): void {
    let x: string | undefined = null as unknown as (string | undefined);
    if (!(x = foo())) {
        x; // string | undefined
    }
    else {
        x; // string
    }
}

function f5(): void {
    let x: string | undefined = null as unknown as (string | undefined);
    let y: string | undefined = null as unknown as (string | undefined);
    if (x = y = foo()) {
        x; // string
        y; // string | undefined
    }
    else {
        x; // string | undefined
        y; // string | undefined
    }
}

/*pruned*/;          
                                                              
                                                              
                               
                                
                    
     
          
                                
                                
     
 

function f7(x: {}): void {
    if (x) {
        x; // {}
    }
    else {
        x; // {}
    }
}

function f8<T>(x: T): void {
    if (x) {
        x; // {}
    }
    else {
        x; // {}
    }
}

/*pruned*/;                                
            
                
     
          
                   
     
 

function main(): void {}
