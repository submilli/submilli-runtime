// @target: es2015
function foo<T>(t: T): void {
    let r1 = t < t;
    let r2 = t > t;
    let r3 = t <= t;
    let r4 = t >= t;
    let r5 = t == t;
    let r6 = t != t;
    let r7 = t === t;
    let r8 = t !== t;
}

function main(): void {}
