// @target: es2015
// @strict: true
// @allowUnreachableCode: true

function f1(): void {
    // Type parameters are in scope in parameters and return types
    /*pruned*/;             
                         
     
}

function f2(): void {
    // Local types are not in scope in parameters and return types
    function f(x: T): T {
        interface T { }
        return undefined;
    }
}

function f3(): void {
    // Type parameters and top-level local types are in same declaration space
    function f<T>(): undefined {
        interface T { }
        return undefined;
    }
}

function f4(): void {
    // Local types are block scoped
    interface T { x: number }
    let v: T = null as unknown as (T);
    v.x = 10;
    if (true) {
        interface T { x: string }
        let v: T = null as unknown as (T);
        v.x = "hello";
    }
    else {
        v.x = 20;
    }
}


function main(): void {}
