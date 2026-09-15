import { accessorBacked, fieldBacked, unrelated } from "@test/geo";

interface Sized {
  size: number;
}

function main(): void {
  const viaAccessor = accessorBacked(5) as Sized;
  assert(viaAccessor.size === 10, "an accessor declared in another package satisfies the cast");

  const viaField = fieldBacked(7) as Sized;
  assert(viaField.size === 7, "a data field from another package still satisfies it");

  let threw = false;
  try {
    const bad = unrelated() as Sized;
    console.log(`${bad.size}`);
  } catch (e) {
    threw = true;
  }
  assert(threw, "a value with no `size` still fails the cast");
}
