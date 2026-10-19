// @target: es2015
// no errors expected when instantiating a generic type with no type arguments provided

class C<T> {
    x: T;
}

let c = new C();

class D<T, U> {
    x: T
    y: U
}

let d = new D();


function main(): void {}
