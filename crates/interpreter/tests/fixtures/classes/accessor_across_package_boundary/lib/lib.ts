export interface Sized { area: number; }
export interface Bag { tag: string; note?: string; }
export interface Perimeter { readonly perimeter: number; }

export function readArea(s: Sized): number { return s.area; }
export function writeArea(s: Sized, v: number): void { s.area = v; }
export function readNote(b: Bag): string | undefined { return b.note; }
export function writeNote(b: Bag, v: string): void { b.note = v; }
export function readPerimeter(p: Perimeter): number { return p.perimeter; }

// Read-modify-write, a chain read, a write-only entry point, and the two
// whole-object operations — all performed by the library, on a shaped receiver
// whose accessor implementation it never sees.
export function bump(s: Sized): void { s.area += 10; }
export function inc(s: Sized): void { s.area++; }
export function chainArea(s: Sized | null): number | undefined { return s?.area; }
export function onlyWrite(s: Sized, v: number): void { s.area = v; }
export function dump(s: Sized): string { return JSON.stringify(s); }
export function same(a: Sized, b: Sized): boolean { return a === b; }

export interface MaybeName { readonly nick?: string; }
export function readNick(m: MaybeName): string | undefined { return m.nick; }
export function chainNick(m: MaybeName | null): string | undefined { return m?.nick; }

// An accessor-backed implementation the consumer cannot name: it reaches the
// consumer only as the interface type the factory returns.
class HiddenSquare implements Sized {
  private side: number = 5;
  get area(): number { return this.side * this.side; }
  set area(v: number) { this.side = v; }
}

export function hiddenSized(): Sized { return new HiddenSquare(); }
