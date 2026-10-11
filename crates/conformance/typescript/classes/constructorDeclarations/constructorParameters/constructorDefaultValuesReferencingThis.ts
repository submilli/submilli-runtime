// @target: es2015
class C {
    public baseProp: number = 1;
    constructor(x = this) { }
}

class D<T> {
    constructor(x = this) { }
}

class E<T> {
    constructor(public x = this) { }
}

class F extends C {
    constructor(y: number = this.baseProp) {
        super();
    }
}


function main(): void {}
