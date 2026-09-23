type GrowingNode<T> = { value: T; next: GrowingNode<T[]> | null; };
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Child extends Parent { value: GrowingNode<number> = { value: 1, next: null }; }
export function main(): void {
 const child = new Child();
 child.reset({ value: 1, next: { value: [2], next: { value: [[3]], next: null } } });
 const valid = child.value;
 assert(valid.value === 1);
 child.reset({ value: 1, next: { value: [2], next: { value: [["bad"]], next: null } } });
 let caught = false;
 try { const invalid = child.value; } catch (error: TypeError) { caught = true; }
 assert(caught);
}
