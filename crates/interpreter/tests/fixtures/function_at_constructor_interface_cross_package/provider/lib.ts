export interface Formatter<T> {
  (value: T): string;
}

export interface Empty {}

export const customBool: BooleanConstructor = (value?: unknown): boolean => value === "yes";

export function makeFormatter<T>(prefix: string): Formatter<T> {
  return (value: T): string => `${prefix}:${String(value)}`;
}

export class ErasedBase {
  slot: unknown = (): void => {};
  boolSlot: unknown = Boolean;
}

export class NarrowedChild extends ErasedBase {
  slot: Empty = (): void => {};
  boolSlot: BooleanConstructor = customBool;
}
