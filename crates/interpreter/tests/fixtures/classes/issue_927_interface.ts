interface Growing<T> { value: T; next: Growing<T[]> | null; read(): number; }
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Child extends Parent { value: Growing<number> = { value: 1, next: null, read: (): number => 1 }; }
export function main(): void {
 const child = new Child();
 child.reset({ value: 1, read: (): number => 1, next: { value: [2], read: (): number => 2, next: { value: [[3]], read: (): number => 3, next: null } } });
 const valid = child.value;
 assert(valid.value === 1);
 child.reset({ value: 1, read: (): number => 1, next: { value: [2], read: (): number => 2, next: { value: [["bad"]], read: (): number => 3, next: null } } });
 let caught = false;
 try { const invalid = child.value; } catch (error: TypeError) { caught = true; }
 assert(caught);
}
