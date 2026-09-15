import { make } from "@test/p3";

function main(): void {
  assert(make().describe() === "token:m", "unreachable — the package fails to compile");
}
