export interface Identified { id: number; }
export interface Holder<T extends Identified> { value: T; }
export type Wrapped<T extends Identified> = { value: T };
export function getId<T extends Identified>(value: T): number { return value.id; }
export class Box<T extends Identified> {
  value: T;
  constructor(value: T) { this.value = value; }
  id(): number { return this.value.id; }
}
