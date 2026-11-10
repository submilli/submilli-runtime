// @target: es2015
let v : Foo<T> = 1;
let v_2 : Foo<T>= 1;

let v_3 : Foo<Bar<T>> = 1;
let v_4 : Foo<Bar<T>>= 1;

let v_5 : Foo<Bar<Quux<T>>> = 1;
let v_6 : Foo<Bar<Quux<T>>>= 1;

function main(): void {}
