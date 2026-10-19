// @strict: true
// @target: es6
class C {
    method?(): void {}
    other(): void {
        this.method?.();
    }
}

function main(): void {}
