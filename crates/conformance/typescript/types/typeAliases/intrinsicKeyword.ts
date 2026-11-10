// @target: es2015
// @strict: true

let e1: intrinsic = null as unknown as (intrinsic);
let e2: { intrinsic: intrinsic } = null as unknown as ({ intrinsic: intrinsic });
type TE1 = (intrinsic);
type TE2<intrinsic> = intrinsic;
type TE3<T extends intrinsic> = T;
type TE4<intrinsic extends intrinsic> = intrinsic;
type TE5<intrinsic extends intrinsic> = (intrinsic);

function f1(): void {
    let intrinsic: intrinsic.intrinsic = null as unknown as (intrinsic.intrinsic);
}

function f2(intrinsic: string): string {
    return intrinsic;
}

function f3(): void {
    type intrinsic = string;
    let s1: intrinsic = 'ok';
}


function main(): void {}
