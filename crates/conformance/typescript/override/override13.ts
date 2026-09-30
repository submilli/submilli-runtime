// @noImplicitOverride: true
// @target: esnext

class Foo {
    property: number = 1
    static staticProperty: number = 2
}

class SubFoo extends Foo {
    property: number = 42;
    staticProperty: number = 42;
}

class StaticSubFoo extends Foo {
    static property: number = 42;
    static staticProperty: number = 42;
}

class Intermediate extends Foo {}

class Derived extends Intermediate {
    property: number = 42;
    staticProperty: number = 42;
}

class StaticDerived extends Intermediate {
    static property: number = 42;
    static staticProperty: number = 42;
}

function main(): void {}
