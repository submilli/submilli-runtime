type Node<T> = { value: T; next: Node<T> | null; };
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Child extends Parent { value: Node<number> = { value: 1, next: null }; }
export function main(): void {
 const cycle: Node<number> = { value: 1, next: null };
 cycle.next = cycle;
 const child = new Child();
 child.reset(cycle);
 assert(child.value.value === 1);
}
