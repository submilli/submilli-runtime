// @target: es2015
// Also note that it is possible to defeat a type guard by calling a function that changes the 
// type of the guarded variable.
function foo(x: number | string): number {
    function f(): void {
        x = 10;
    }
    if (typeof x === "string") {
        f();
        return x.length; // string
    }
    else {
        return x++; // number
    }
}
function foo2(x: number | string): number | undefined {
    if (typeof x === "string") {
        return x.length; // string
    }
    else {
        let f = function () {
            return x * x;
        };
    }
    x = "hello";
    ;   
}
function foo3(x: number | string): number | undefined {
    if (typeof x === "string") {
        return x.length; // string
    }
    else {
        let f = () => x * x;
    }
    x = "hello";
    ;   
}


function main(): void {}
