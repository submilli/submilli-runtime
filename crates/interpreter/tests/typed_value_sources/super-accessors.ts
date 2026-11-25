class Base {
    private v: number = 1;
    private opt: number | undefined = undefined;
    factor(): number {
        return 1;
    }
    get x(): number {
        return this.v * this.factor();
    }
    set x(v: number) {
        this.v = v;
    }
    get raw(): number {
        return this.v;
    }
    get maybe(): number | undefined {
        return this.opt;
    }
    set maybe(v: number | undefined) {
        this.opt = v;
    }
    get pureNum(): number {
        return 42;
    }
    get pureFlag(): boolean {
        return true;
    }
    get reader(): (this: Base) => number {
        return function (this: Base): number {
            return this.raw;
        };
    }
}

class Mid extends Base {
    override factor(): number {
        return 2;
    }
    override get x(): number {
        return super.x * 10;
    }
    override set x(v: number) {
        super.x = v + 1;
    }
    readSuperArrow(): () => number {
        return () => super.x;
    }
    readSuperNestedArrow(): () => () => number {
        return () => () => super.x;
    }
    writeSuperArrow(): (v: number) => void {
        return (v: number) => {
            super.x = v;
        };
    }
    writeSuperNestedArrow(): () => (v: number) => void {
        return () => (v: number) => {
            super.x = v;
        };
    }
    assignExpr(v: number): number {
        const assigned = (super.x = v);
        return assigned;
    }
    callSuperGetterFn(): number {
        return super.reader();
    }
    readPure(): string {
        return String(super.pureNum) + ":" + String(super.pureFlag);
    }
    bumpSuper(): number {
        super.x += 3;
        const prev = super.x++;
        return prev;
    }
    fillMaybe(fallback: () => number): number {
        super.maybe ??= fallback();
        return super.maybe ?? -1;
    }
}

class Leaf extends Mid {
    override get x(): number {
        return super.x + 1000;
    }
    override set x(v: number) {
        super.x = v * 2;
    }
    writeBaseDirect(v: number): void {
        super.maybe = v;
    }
}

class Box<T> {
    private item: T;
    constructor(item: T) {
        this.item = item;
    }
    get value(): T {
        return this.item;
    }
    set value(v: T) {
        this.item = v;
    }
}

class NumBox extends Box<number> {
    constructor(n: number) {
        super(n);
        super.value = super.value + 5;
    }
    override get value(): number {
        return super.value * 3;
    }
    override set value(v: number) {
        super.value = v - 1;
    }
}

class ObjBox extends Box<{ a: number }> {
    override get value(): { a: number } {
        const inner = super.value;
        return { a: inner.a + 1 };
    }
    override set value(v: { a: number }) {
        super.value = { a: v.a * 2 };
    }
}

function main(): string {
    const out: string[] = [];

    const m = new Mid();
    out.push(String(m.x));
    m.x = 4;
    out.push(String(m.raw));
    out.push(String(m.x));
    out.push(String(m.readSuperArrow()()));
    out.push(String(m.readSuperNestedArrow()()()));
    m.writeSuperArrow()(7);
    out.push(String(m.raw));
    m.writeSuperNestedArrow()()(8);
    out.push(String(m.raw));
    out.push(String(m.assignExpr(7)));
    out.push(String(m.callSuperGetterFn()));
    out.push(m.readPure());
    const prev = m.bumpSuper();
    out.push(String(prev));
    out.push(String(m.raw));

    let calls = 0;
    const first = m.fillMaybe(() => {
        calls = calls + 1;
        return 42;
    });
    const second = m.fillMaybe(() => {
        calls = calls + 1;
        return 99;
    });
    out.push(String(first));
    out.push(String(second));
    out.push(String(calls));

    const l = new Leaf();
    out.push(String(l.x));
    l.x = 3;
    out.push(String(l.raw));
    out.push(String(l.x));
    l.writeBaseDirect(123);
    out.push(String(l.maybe));

    const nb = new NumBox(10);
    out.push(String(nb.value));
    nb.value = 20;
    out.push(String(nb.value));

    const ob = new ObjBox({ a: 5 });
    out.push(String(ob.value.a));
    ob.value = { a: 7 };
    out.push(String(ob.value.a));

    return out.join("|");
}
