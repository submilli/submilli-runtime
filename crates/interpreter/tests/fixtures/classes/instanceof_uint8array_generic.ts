function looksLikeBytes<T>(x: T): boolean {
  return x instanceof Uint8Array;
}

function main(): void {
  assert(looksLikeBytes<Uint8Array>(new Uint8Array([1, 2])), "erased bytes");
  assert(looksLikeBytes<Uint8Array>(new Uint8Array([])), "empty erased bytes");
  assert(!looksLikeBytes<string>("bytes"), "erased string is not bytes");
  assert(!looksLikeBytes<number[]>([1, 2]), "erased array is not bytes");
  assert(!looksLikeBytes<null>(null), "erased null is not bytes");
}
