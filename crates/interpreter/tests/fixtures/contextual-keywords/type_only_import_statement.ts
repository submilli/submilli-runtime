// `import type` is accepted as a synonym for a plain import: our imports carry
// both value and type spaces, so type-only has no semantic difference here.
import type { v4, validate } from "submilli:uuid";

function main(): void {
  assert(validate(v4()));
}
