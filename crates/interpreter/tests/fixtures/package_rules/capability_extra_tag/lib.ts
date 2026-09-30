// expect-warning: extra `@capability test.com/op` has no matching `check()` call
// expect-error-count: 1

/**
 * Runs the operation.
 * @capability test.com/op { id }
 */
export function run(id: string): void {}
