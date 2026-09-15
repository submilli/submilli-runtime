import { set as store, get as load } from "submilli:session";

function main(): void {
  store("k", "v");
  assert(load("k") as string === "v", "aliased import reads and writes");
}
