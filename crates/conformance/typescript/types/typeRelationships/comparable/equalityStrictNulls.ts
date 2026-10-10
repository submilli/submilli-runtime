// @target: es2015
// @strictNullChecks: true

function f1(x: string): void {
    if (x == undefined) {
    }
    if (x != undefined) {
    }
    if (x === undefined) {
    }
    if (x !== undefined) {
    }
    if (x == null) {
    }
    if (x != null) {
    }
    if (x === null) {
    }
    if (x !== null) {
    }
    if (undefined == x) {
    }
    if (undefined != x) {
    }
    if (undefined === x) {
    }
    if (undefined !== x) {
    }
    if (null == x) {
    }
    if (null != x) {
    }
    if (null === x) {
    }
    if (null !== x) {
    }
}

function f2(): void {
    if (undefined == undefined) {
    }
    if (undefined == null) {
    }
    if (null == undefined) {
    }
    if (null == null) {
    }
}

function f3(a: number, b: boolean, c: { x: number }, d: number | string): void {
    if (a == null) {
    }
    if (b == null) {
    }
    if (c == null) {
    }
    if (d == null) {
    }
}

function f4(x: number): void {
    if (x > undefined) {
    }
    if (x < undefined) {
    }
    if (x >= undefined) {
    }
    if (x <= undefined) {
    }
}
function f5(x: string): void {
    switch(x) {
        case null:
            break;
        case undefined:
            break;
        default:
            return;
    }
}


function main(): void {}
