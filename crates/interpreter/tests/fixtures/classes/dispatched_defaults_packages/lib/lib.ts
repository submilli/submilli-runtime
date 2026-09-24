export class Base {
    greet(name: string = "base"): string { return name; }
}
export class Derived extends Base {
    greet(name: string = "derived"): string { return name; }
}
export function make(): Base { return new Derived(); }
export function call(value: Base): string { return value.greet(); }
export interface Value { a?: string | null }
export function absent(): Value { return {}; }
export function present(): Value { return { a: null }; }
export function write(value: Value): void { value.a = null; }
export function reader(): () => number {
    return function(this: { value: number }): number { return this.value; };
}
