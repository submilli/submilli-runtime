// expect-warning: parameter `count` is undocumented (missing `@param count`)
// expect-warning: missing `@returns` on a non-`void`-returning function
// expect-warning: `@param size` does not match any parameter of this function
// expect-warning: destructured parameter 1 is undocumented (add a `@param` in its position)
// expect-warning: destructured method parameter 1 is undocumented (add a `@param` in its position)
// expect-error-count: 5

/**
 * Repeats a word.
 * @param word Word to repeat.
 */
export function repeat(word: string, count: number): string {
  return word.repeat(count);
}

/**
 * Logs nothing.
 * @param size Ignored.
 */
export function quiet(): void {}

/**
 * Doubles a number.
 * @param n Number to double.
 * @returns Twice `n`.
 */
export function double(n: number): number {
  return n * 2;
}

/**
 * Adds a pair; the `@param` in a destructured parameter's position documents it.
 * @param pair Numbers to add.
 * @returns Their sum.
 */
export function add({ a, b }: { a: number; b: number }): number {
  return a + b;
}

/**
 * Subtracts a pair.
 * @returns Their difference.
 */
export function subtract({ a, b }: { a: number; b: number }): number {
  return a - b;
}

/** A shape. */
export interface Shape {
  /**
   * Area at a scale; an interface method's destructured parameter is documented the same way.
   * @param options Scale to apply.
   * @returns The scaled area.
   */
  area({ scale }: { scale: number }): number;
  /**
   * Perimeter at a scale.
   * @returns The scaled perimeter.
   */
  perimeter({ scale }: { scale: number }): number;
}
