class Base { greet(name: string = "base"): string { return "B:" + name; } }
class Derived extends Base { greet(name: string = "derived"): string { return "D:" + name; } }
function greetNullable(d: Base | null): string | null { return d?.greet() ?? null; }
function main(): void {
 const d: Base = new Derived();
 const dd: Derived = new Derived();
 assert(d.greet() === "D:derived", "base reference uses override default");
 assert(greetNullable(d) === "D:derived", "optional chain default");
 assert(greetNullable(null) === null, "optional chain skips null");
 assert(dd.greet() === "D:derived", "derived default");
}
