type Swap<Z, A> = { z: Z; a: A; next: Swap<A, Z[]> | null; };
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Child extends Parent { value: Swap<number, string> = { z: 1, a: "a", next: null }; }
export function main(): void {
 const child = new Child();
 child.reset({ z: 1, a: "a", next: { z: "b", a: [2], next: { z: [3], a: ["c"], next: null } } });
 assert(child.value.z === 1);
 child.reset({ z: 1, a: "a", next: { z: "b", a: [2], next: { z: ["bad"], a: ["c"], next: null } } });
 let caught = false;
 try { const invalid = child.value; } catch (error: TypeError) { caught = true; }
 assert(caught);
}
