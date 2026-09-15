// expect-error: field `sound` does not exist
import { Animal } from "@test/zoo3";

// `sound` is private to `Animal`'s package; a consumer cannot read it even
// though the imported rec group physically contains the slot.
function main(): void {
  const a = new Animal("Rex", "woof");
  const leaked: string = a.sound;
  assert(leaked === "woof");
}
