class Animal { speak(): string { return "animal"; } }
class Dog extends Animal { fetch(): string { return "dog"; } }
class Base<T> { value: T | Animal | null = null; reset(x: T | Animal | null): void { this.value = x; } }
class Child<T> extends Base<T> { value: T | null = null; read(): T | null { return this.value; } }
function make<T>(): Child<T> { return new Child<T>(); }
export function main(): void {
 const c = make<Dog>(); c.reset(new Animal());
 let caught = false;
 try { const value = c.read(); } catch(e) { caught = e instanceof TypeError; }
 assert(caught, "factory created generic class uses concrete validation");
 const empty = new Child<string | null>();
 assert(empty.read() === null, "nullable generic argument stays nullable");
 assert(Object.hasOwn(empty, "guard value") === false, "guards are not properties");
 const keys = Object.keys(empty);
 assert(keys.length === 3, "only value and method slots are named");
}
