import { get, set, has } from "submilli:session";

function main(): void {
  set("k", 1);
  assert(has("k"), "named import writes");
  assert(get("k") as number === 1, "named import reads");
}
