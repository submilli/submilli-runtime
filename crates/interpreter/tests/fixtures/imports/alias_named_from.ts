import { v4 as from, validate } from "submilli:uuid";

function main(): void {
  assert(validate(from()), "import alias named `from` is a legal binding");
}
