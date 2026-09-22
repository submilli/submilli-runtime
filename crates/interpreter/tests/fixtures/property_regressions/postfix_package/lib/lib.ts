export interface Counter { value: bigint; }
export function inc(counter: Counter): bigint { return counter.value++; }
export function dec(counter: Counter): bigint { return counter.value--; }
export interface OptionalValue { optional?: string; }
export class OptionalBase { optional?: string; }
export function hasOptional(value: unknown): boolean { return "optional" in value; }
