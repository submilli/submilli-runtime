// Adapted by hand from the upstream case, for its compound assignments: it is no
// longer about private names. The `#field` private name is a `private` field,
// since Submilli has no `#` names; the field and `getInstance` have the types
// Submilli requires; and the bitwise compound assignments are removed, since
// Submilli doesn't support bitwise operators yet.
// @target: es2015

class A {
    private field: number = 0;
    constructor() {
        this.field = 1;
        this.field += 2;
        this.field -= 3;
        this.field /= 4;
        this.field *= 5;
        this.field **= 6;
        this.field %= 7;
        A.getInstance().field = 1;
        A.getInstance().field += 2;
        A.getInstance().field -= 3;
        A.getInstance().field /= 4;
        A.getInstance().field *= 5;
        A.getInstance().field **= 6;
        A.getInstance().field %= 7;
    }
    static getInstance(): A {
        return new A();
    }
}

function main(): void {}
