import { collect, pair } from "@test/spread";

function main(): void {
  const page = [1, 2, 3];
  assert(collect(0, ...page, 4).join(",") === "0,1,2,3,4", "package rest spread");
  const tuple: [number, string] = [7, "x"];
  assert(pair(...tuple) === "7:x", "package fixed tuple spread");
}
