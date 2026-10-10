class Box<T> { value: T; constructor(value: T) { this.value = value; } }
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Nested extends Parent { value: Box<Box<string>> = new Box<Box<string>>(new Box<string>("nested")); }
class Child extends Parent { value: Box<string> = new Box<string>("ok"); }
export function main(): void {
 const nested = new Nested();
 assert(nested.value.value.value === "nested", "finite nested class arguments pass");
 const child = new Child(); child.reset(new Box<number>(1));
 let caught = false;
 try { const box = child.value; } catch (e) { caught = e instanceof TypeError; }
 assert(caught, "generic payload mismatch is caught at the narrowed read");
}
