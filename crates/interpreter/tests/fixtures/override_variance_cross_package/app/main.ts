import { Base } from "@test/base";
import { Child } from "@test/child";
interface Picker { pick(): string | number; }
function main(): void {
 const c = new Child();
 const b: Base = c;
 const i: Picker = c;
 c.x = null;
 assert(c.n === -1, "imported widened setter");
 c.put(9);
 assert(c.n === 9, "imported mixed parameter");
 b.put("abc");
 assert(c.n === 3, "imported base call");
 assert(c.pick() === 42, "direct narrow result");
 assert(b.pick() === 42, "base narrow result");
 assert(i.pick() === 42, "interface narrow result");
}
