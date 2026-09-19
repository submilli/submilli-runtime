class Animal {}
class Dog extends Animal {}
class Root { value: unknown = null; set(value: unknown): void { this.value = value; } }
class Middle extends Root { value: Animal | null = null; read(): Animal | null { return this.value; } }
class Child extends Middle { value: Dog | null = null; }
export function main(): void { const child = new Child(); child.set(new Animal()); assert(child.read() instanceof Animal, "middle read admits Animal"); }
