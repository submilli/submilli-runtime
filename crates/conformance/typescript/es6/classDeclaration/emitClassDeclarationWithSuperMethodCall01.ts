//@target: es6

class Parent {
    foo(): void {
    }
}

class Foo extends Parent {
    foo(): void {
        let x = () => super.foo();
    }
}

function main(): void {}
