function applyMarker(f: (x: number) => number, receiver: number, x: number): number {
    return Reflect.apply(f, receiver, [x]);
}

function nullMarker(f: () => boolean, receiver: unknown): boolean {
    return Reflect.apply(f, receiver, []);
}

function main(): number {
    let trace = 0;
    const f = function(this: number, x: number): number { return this + x; };
    const getCallee = () => { trace = trace * 10 + 1; return f; };
    const getReceiver = (): number => { trace = trace * 10 + 2; return 40; };
    const getArgument = (): number => { trace = trace * 10 + 3; return 2; };
    const result = applyMarker(getCallee(), getReceiver(), getArgument());
    const isNull = function(this: unknown): boolean { return this === null; };
    if (!nullMarker(isNull, null)) return -1;
    if (nullMarker(isNull, undefined)) return -2;
    const holder = { f };
    if (applyMarker(holder.f, 5, 2) !== 7) return -3;
    const fail = (): number => { trace = trace * 10 + 4; throw new TypeError("receiver"); };
    try {
        applyMarker(getCallee(), fail(), getArgument());
        return -4;
    } catch (e) {
        if (!(e instanceof TypeError)) return -5;
    }
    return trace * 100 + result;
}
