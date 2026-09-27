// @target: es2015
// @strictNullChecks: true

function foo(): string | null { return null as unknown as (string | null); }

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
    let x: string | null = null as unknown as (string | null);
    x = foo();
    if (x) {
        x; // string
    }
    else {
        x; // string | undefined
    }
}

function f3(): void {
    let x: string | null = null as unknown as (string | null);
    if (x = foo()) {
        x; // string
    }
    else {
        x; // string | undefined
    }
}

function f4(): void {
    let x: string | null = null as unknown as (string | null);
    if (!(x = foo())) {
        x; // string | undefined
    }
    else {
        x; // string
    }
}

function f5(): void {
    let x: string | null = null as unknown as (string | null);
    let y: string | null = null as unknown as (string | null);
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
