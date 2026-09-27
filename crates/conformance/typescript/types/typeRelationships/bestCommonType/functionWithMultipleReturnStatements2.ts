// @target: es2015
// @allowUnreachableCode: true

// return type of a function with multiple returns is the BCT of each return statement
// no errors expected here

function f1(): 1 | null {
    if (true) {
        return 1;
    } else {
        return null;
    }
}

function f2(): 1 | 2 | null {
    if (true) {
        return 1;
    } else if (false) {
        return null;
    } else {
        return 2;
    }
}

function f4(): 1 | null {
    try {
        return 1;
    }
    catch (e) {
        return null;
    }
    finally {
        return 1;
    }
}

/*pruned*/;                
             
                        
 

function f6<T>(x: T): T | null {
    if (true) {
        return x;
    } else {
        return null;
    }
}

//function f7<T extends U, U>(x: T, y: U) {
//    if (true) {
//        return x;
//    } else {
//        return y;
//    }
//}

let a: { x: number; y?: number } = null as unknown as ({ x: number; y?: number });
let b: { x: number; z?: number } = null as unknown as ({ x: number; z?: number });
// returns typeof a
function f9(): { x: number; y?: number; } | { x: number; z?: number; } {
    if (true) {
        return a;
    } else {
        return b;
    }
}

// returns typeof b
function f10(): { x: number; y?: number; } | { x: number; z?: number; } {
    if (true) {
        return b;
    } else {
        return a;
    }
}

// returns number => void
function f11(): ((x: number) => void) | ((x: Object) => void) {
    if (true) {
        return (x: number) => { }
    } else {
        return (x: Object) => { }
    }
}

// returns Object => void
function f12(): ((x: Object) => void) | ((x: number) => void) {
    if (true) {
        return (x: Object) => { }
    } else {
        return (x: number) => { }        
    }
}

function main(): void {}
