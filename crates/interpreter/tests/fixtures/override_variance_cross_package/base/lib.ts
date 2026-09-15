export class Base { n: number = 0; pick(): string | number { return "base"; } set x(v: number) { this.n = v; } put(v: string): void { this.n = v.length; } }
