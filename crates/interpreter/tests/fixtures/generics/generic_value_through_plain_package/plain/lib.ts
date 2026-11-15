/**
 * Calls `f` with `key`.
 * @param f The function.
 * @param key The key.
 * @returns What it returned.
 */
export function callWith(f: (key: string) => number | undefined, key: string): number | undefined {
  return f(key);
}

/**
 * Reads through `read`.
 * @param read The reader.
 * @param key The key.
 * @returns What it read.
 */
export function viaReader(read: (key: string) => number | undefined, key: string): number | undefined {
  return read(key);
}
