export function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

export function assertPresent<T>(x: T | null | undefined): asserts x {
  if (x === null || x === undefined) throw new Error("missing");
}

export const alias: typeof assertIsString = assertIsString;
