// @target: es2015
class C extends A<T> implements B<T> {
}

let v1: C<T> = null as unknown as (C<T>);
let v2: D<T> = null;
let v3: E.F<T> = null as unknown as (E.F<T>);
let v3_2: G.H.I<T> = null as unknown as (G.H.I<T>);
let v6: K<T>[] = null as unknown as (K<T>[]);


function f1(a: E<T>): void {
}

function f2(): F<T> {
}



function main(): void {}
