// expect-error: field `sound` does not exist
import { Animal } from "./animal";

// `sound` is private to module `animal`; reading it from another module is a
// "no such field" error (module-scoped privacy, docs/classes.md §2-3).
export function noise(a: Animal): string {
  return a.sound;
}
