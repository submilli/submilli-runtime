// @target: es2015
// @strictNullChecks: true
// @declaration: true


function error(message: string): never {
    throw new Error(message);
}

function errorVoid(message: string): void {
    throw new Error(message);
}

function fail(): never {
    return error("Something failed");
}

function failOrThrow(shouldFail: boolean): never {
    if (shouldFail) {
        return fail();
    }
    throw new Error();
}

function infiniteLoop1(): void {
    while (true) {
    }
}

function infiniteLoop2(): never {
    while (true) {
    }
}

/*pruned*/;                                       
                        
                  
                     
                    
                       
     
                                          
 

/*pruned*/;                                       
                                   
                                   
                                       
 

/*pruned*/;                                     
                                         
 

class C {
    void1(): void {
        throw new Error();
    }
    void2(): void {
        while (true) {}
    }
    never1(): never {
        throw new Error();
    }
    never2(): never {
        while (true) {}
    }
}

function f1(x: string | number): void {
    if (typeof x === "boolean") {
        x;  // never
    }
}

function f2(x: string | number): never {
    while (true) {
        if (typeof x === "boolean") {
            return x;  // never
        }
    }
}

function test(cb: () => string): string {
    let s = cb();
    return s;
}

let errorCallback = () => error("Error callback");

test(() => "hello");
test(() => fail());
test(() => { throw new Error(); })
test(errorCallback);


function main(): void {}
