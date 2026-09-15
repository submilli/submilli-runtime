// A self-referential generic class: a `Node<T> | null` field, methods that
// build and walk the chain, and null-narrowing on the erased link.
class Node<T> {
  value: T;
  next: Node<T> | null = null;

  constructor(v: T) {
    this.value = v;
  }

  push(v: T): Node<T> {
    const n = new Node(v);
    this.next = n;
    return n;
  }

  length(): number {
    let count = 1;
    let cur: Node<T> | null = this.next;
    while (cur !== null) {
      count = count + 1;
      cur = cur.next;
    }
    return count;
  }
}

function main(): void {
  const head = new Node(1);
  head.push(2).push(3);
  assert(head.length() === 3, "walk a recursive generic chain");
  assert(head.next !== null && head.next.value === 2, "narrowed link field");

  const s = new Node("a");
  s.push("b");
  assert(s.length() === 2, "same shape at a string instantiation");
}
