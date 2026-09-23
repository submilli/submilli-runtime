// @target: es2015
// The && operator permits the operands to be of any type and produces a result of the same
// type as the second operand.

function foo<T, U, V/* extends T*/>(t: T, u: U, v: V): void {
    let r1 = t && t;
    let r2 = u && t;
    let r3 = v && t;

    let r4 = t && u;
    let r5 = u && u;
    let r6 = v && u;

    let r7 = t && v;
    let r8 = u && v;
    let r9 = v && v;

    let a: number = null as unknown as (number);
    let r10 = t && a;
}

function main(): void {}
