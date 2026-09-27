// @target: es2015
// @strictNullChecks: true

function f1(x: string): void {
    if (x == null) {
    }
    if (x != null) {
    }
    if (x === null) {
    }
    if (x !== null) {
    }
    if (x == null) {
    }
    if (x != null) {
    }
    if (x === null) {
    }
    if (x !== null) {
    }
    if (null == x) {
    }
    if (null != x) {
    }
    if (null === x) {
    }
    if (null !== x) {
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
    if (null == null) {
    }
    if (null == null) {
    }
    if (null == null) {
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
    if (x > null) {
    }
    if (x < null) {
    }
    if (x >= null) {
    }
    if (x <= null) {
    }
}
function f5(x: string): void {
    switch(x) {
        case null:
            break;
        case null:
            break;
        default:
            return;
    }
}


function main(): void {}
