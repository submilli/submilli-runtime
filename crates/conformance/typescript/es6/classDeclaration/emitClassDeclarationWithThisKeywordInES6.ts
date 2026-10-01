// @target: es6
class B {
    x: number = 10;
    constructor() {
        this.x = 10;
    }
    static log(a: number): void { }
    foo(): void {
        B.log(this.x);
    }

    get X(): number {
        return this.x;
    }

    set bX(y: number) {
        this.x = y;
    }
}

function main(): void {}
