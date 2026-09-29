// @target: es2015
class C extends A<X<T>, Y<Z<T>>> implements B<X<T>, Y<Z<T>>> {
}

let v1: C<X<T>, Y<Z<T>>> = null as unknown as (C<X<T>, Y<Z<T>>>);
let v2: D<X<T>, Y<Z<T>>> = null;
let v3: E.F<X<T>, Y<Z<T>>> = null as unknown as (E.F<X<T>, Y<Z<T>>>);
let v4: G.H.I<X<T>, Y<Z<T>>> = null as unknown as (G.H.I<X<T>, Y<Z<T>>>);
let v6: K<X<T>, Y<Z<T>>>[] = null as unknown as (K<X<T>, Y<Z<T>>>[]);


function f1(a: E<X<T>, Y<Z<T>>>): void {
}

function f2(): F<X<T>, Y<Z<T>>> {
}



function main(): void {}
