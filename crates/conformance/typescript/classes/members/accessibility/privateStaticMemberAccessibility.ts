// @target: es2015
class Base {
    private static foo: string;
}

class Derived extends Base {
    static bar: string = Base.foo; // error
    bing: () => string = () => Base.foo; // error
}

function main(): void {}
