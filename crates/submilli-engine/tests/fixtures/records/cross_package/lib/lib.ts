export interface Values<T> { [key: string]: T; }
export function make(): Values<number> { return { count: 1 }; }
export function put(values: Values<number>, key: string, value: number): void { values[key] = value; }
export function read(values: Values<number>, key: string): number | undefined { return values[key]; }

export function readOptional(values: Values<number> | null): number | undefined { return values?.count; }
