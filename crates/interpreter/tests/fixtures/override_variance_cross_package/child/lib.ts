import { Base } from "@test/base";
export class Child extends Base { pick(): number { return 42; } set x(v: number | null) { this.n = v === null ? -1 : v; } put(v: string | number): void { this.n = typeof v === "number" ? v : v.length; } }
