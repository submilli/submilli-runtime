import { looksLikeBytes, makeBytes } from "@test/bytes";

function main(): void {
  assert(looksLikeBytes(new Uint8Array([1, 2])), "consumer-built bytes pass the producer's guard");
  assert(!looksLikeBytes("bytes"), "producer's guard still rejects a string");
  assert(!looksLikeBytes(null), "producer's guard still rejects null");

  // ...and the reverse direction: producer-built bytes tested in the consumer.
  const fromPackage: unknown = makeBytes();
  assert(fromPackage instanceof Uint8Array, "producer-built bytes pass the consumer's test");
}
