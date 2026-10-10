import { NAME, XS, COUNT } from "@test/cfg";

function main(): void {
  assert(NAME === "hi");
  assert(XS.length === 3);
  assert(XS[1] === 2);
  assert(COUNT === 7);
}
