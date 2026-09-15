// `Noted` is an *unrelated* class, not an implementation of `Bag` — that is the
// whole point of this file, and what distinguishes it from
// `interface_property_write_dispatch.ts`, where every class implements the
// interface under test. Merely declaring `get note` / `set note` anywhere in the
// program is what makes a `Bag` property access emit its accessor branch, so an
// absent optional read or write must not be steered into a getter or setter that
// belongs to a type it has nothing to do with.
interface Bag {
  tag: string;
  note?: string;
}

class Noted {
  private value: string = "";
  get note(): string {
    return this.value;
  }
  set note(v: string) {
    this.value = v;
  }
}

function main(): void {
  const noted = new Noted();
  noted.note = "accessor";
  assert(noted.note === "accessor", "the sibling accessor still works");

  const raw = { tag: "b" };
  const absent: Bag = raw;
  assert(absent.note === null, "absent optional property reads null");

  // The write has nowhere to go, but must not dispatch the sibling's setter.
  absent.note = "ignored";
  assert(absent.note === null, "absent optional property stays absent");
  assert(noted.note === "accessor", "the write did not reach the other object");
}
