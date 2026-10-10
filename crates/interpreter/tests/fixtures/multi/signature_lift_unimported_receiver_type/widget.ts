export class Box<T> {
    private v: T;
    constructor(v: T) { this.v = v; }
    put(x: T): void { this.v = x; }
}

export class StringBox extends Box<string> {}

export class Pair<K, V> {
    private k: K;
    private v: V;
    constructor(k: K, v: V) { this.k = k; this.v = v; }
    set(k: K, v: V): void { this.k = k; this.v = v; }
}

export class Flip<A, B> extends Pair<B, A> {}

export function makeBox(): Box<string> { return new Box<string>("a"); }
export function makeStringBox(): StringBox { return new StringBox("a"); }
export function makeFlip(): Flip<string, number> { return new Flip<string, number>(1, "a"); }
