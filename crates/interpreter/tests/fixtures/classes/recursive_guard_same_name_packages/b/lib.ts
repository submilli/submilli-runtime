export type Node = { b: string; next: Node | null };

export class Parent {
  value: unknown = null;
  reset(value: unknown): void { this.value = value; }
}

export class Child extends Parent {
  value: Node = { b: "ok", next: null };
}
