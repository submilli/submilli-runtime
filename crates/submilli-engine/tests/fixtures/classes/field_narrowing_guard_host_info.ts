import { info, Info } from "submilli:fs";
import { parse } from "submilli:url";
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Child extends Parent { value: Info = info(); }
export function main(): void {
 const c = new Child(); assert(c.value.mode.length > 0, "valid Info is accepted");
 c.reset(parse("https://example.com"));
 let caught = false;
 try { const value = c.value; } catch (e) { caught = e instanceof TypeError; }
 assert(caught, "URL is rejected as Info");
}
