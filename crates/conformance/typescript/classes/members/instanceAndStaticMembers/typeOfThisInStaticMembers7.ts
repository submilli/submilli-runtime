// @target: esnext, es2022, es6, es5

class C {
    static a: number = 1;
    static b: number = this.a + 1;
}

class D extends C {
    static c: number = 2;
    static d: number = this.c + 1;
    static e: number = 1 + (super.a) + (this.c + 1) + 1;
}


function main(): void {}
