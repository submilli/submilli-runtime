import { Probe } from "@test/probe";

function main(): void {
  const p = new Probe();
  assert(JSON.stringify(p) === "{\"foo\":1}", "package class toJson literal");
  assert(p.describe() === "{\"kind\":\"probe\",\"n\":0}", "package class method literal");
}
