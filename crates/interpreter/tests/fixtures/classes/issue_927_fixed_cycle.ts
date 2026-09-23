type Fixed<T> = { value: T; next: Fixed<string> | null; };
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Child extends Parent { value: Fixed<string> = { value: "ok", next: null }; }
export function main(): void {
 const node: Fixed<string> = { value: "ok", next: null };
 node.next = node;
 const child = new Child();
 child.reset(node);
 assert(child.value.value === "ok");
}
