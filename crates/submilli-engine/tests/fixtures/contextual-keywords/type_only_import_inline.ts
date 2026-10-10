import { type v4, validate } from "submilli:uuid";

function main(): void {
  assert(validate(v4()));
}
