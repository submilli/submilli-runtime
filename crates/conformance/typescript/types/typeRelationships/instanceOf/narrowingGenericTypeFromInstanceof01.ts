// @target: es2015
class A<T> {
    constructor(private a: string) { }
}

class B<T> {
}

function acceptA<T>(a: A<T>): void { }
function acceptB<T>(b: B<T>): void { }

function test<T>(x: A<T> | B<T>): void {
    if (x instanceof B) {
        acceptA(x);
    }

    if (x instanceof A) {
        acceptA(x);
    }

    if (x instanceof B) {
        acceptB(x);
    }

    if (x instanceof B) {
        acceptB(x);
    }
}

function main(): void {}
