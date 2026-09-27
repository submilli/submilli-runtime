import { Base } from "@test/json-base";
class Child extends Base { name: string = "child"; }
function main(): void {
  assert(JSON.stringify(new Child()) === '{"name":"child","value":4}', "imported private members stay private");
}
