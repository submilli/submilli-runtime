// @target: es2015
function getStringOrNumber(): string | number { return null as unknown as (string | number); }

function f1(): void {
    const x = getStringOrNumber();
    if (typeof x === "string") {
        const f = () => x.length;
    }
}

function f2(): void {
    const x = getStringOrNumber();
    if (typeof x !== "string") {
        return;
    }
    const f = () => x.length;
}

function f3(): void {
    const x = getStringOrNumber();
    if (typeof x === "string") {
        const f = function() { return x.length; };
    }
}

function f4(): void {
    const x = getStringOrNumber();
    if (typeof x !== "string") {
        return;
    }
    const f = function() { return x.length; };
}

function f5(): void {
    const x = getStringOrNumber();
    if (typeof x === "string") {
        const f = () => () => x.length;
    }
}

function main(): void {}
