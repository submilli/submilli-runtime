// The guard runs in this package's module; the bytes it inspects are built in
// `main`'s module. Both sides declare their own `$Uint8Array`, which structural
// canonicalization makes the same type.
export function looksLikeBytes(x: unknown): boolean {
  return x instanceof Uint8Array;
}

export function makeBytes(): Uint8Array {
  return new Uint8Array([9, 8, 7]);
}
