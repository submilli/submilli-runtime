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

function move1(direction: "up" | "down"): 1 | -1 {
    switch (direction) {
        case "up":
            return 1;
        case "down":
            return -1; 
    }
    return error("Should never get here");
}

function move2(direction: "up" | "down"): 1 | -1 {
    return direction === "up" ? 1 :
        direction === "down" ? -1 :
        error("Should never get here");
}

function check<T>(x: T | undefined): NonNullable<T> {
    return x || error("Undefined value");
}

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
