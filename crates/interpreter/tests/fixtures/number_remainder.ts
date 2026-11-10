function remainder(left: number, right: number): number { return left % right; }
let order = "";
function operand(label: string, value: number): number { order += label; return value; }
const box = { value: -1 };
function receiver(): { value: number } { order += "o"; return box; }
const items = [-1];
function array(): number[] { order += "a"; return items; }
function index(): number { order += "i"; return 0; }

function main(): void {
    assert(Object.is(remainder(-1, 1), -0));
    assert(Object.is(remainder(-0, 5), -0));
    assert(Object.is(remainder(0, -5), 0));
    assert(Object.is(remainder(-0, Infinity), -0));
    assert(remainder(5, Infinity) === 5);
    assert(remainder(-5, -Infinity) === -5);
    assert(remainder(1e17, 3) === 1);
    assert(remainder(1e308, 1e-308) === 3.498445546245627e-309);
    assert(remainder(-10, 3) === -1);
    assert(remainder(10, -3) === 1);
    assert(remainder(-10, -3) === -1);
    assert(remainder(5.5, 2) === 1.5);
    assert(remainder(1.5e-323, 1e-323) === 5e-324);
    assert(Object.is(remainder(-1e-323, 5e-324), -0));
    assert(remainder(2.2250738585072014e-308, 2.225073858507201e-308) === 5e-324);
    assert(remainder(1.7976931348623157e308, 5e-324) === 0);
    const special = [NaN, Infinity, -Infinity, 0, -0];
    for (const value of special) {
        assert(Number.isNaN(remainder(NaN, value)));
        assert(Number.isNaN(remainder(value, NaN)));
        assert(Number.isNaN(remainder(Infinity, value)));
        assert(Number.isNaN(remainder(-Infinity, value)));
        assert(Number.isNaN(remainder(value, 0)));
        assert(Number.isNaN(remainder(value, -0)));
    }
    assert(Number.isNaN(remainder(1, 0)));
    assert(Number.isNaN(remainder(1, -0)));

    assert(Object.is(operand("l", -1) % operand("r", 1), -0));
    assert(order === "lr");
    order = "";
    let local = -1;
    local %= operand("r", 1);
    assert(Object.is(local, -0));
    assert(order === "r");
    order = "";
    assert(Object.is(receiver().value % operand("r", 1), -0));
    assert(order === "or");
    order = "";
    receiver().value %= operand("r", 1);
    assert(Object.is(box.value, -0));
    assert(order === "or");
    order = "";
    assert(Object.is(array()[index()] % operand("r", 1), -0));
    assert(order === "air");
    order = "";
    array()[index()] %= operand("r", 1);
    assert(Object.is(items[0], -0));
    assert(order === "air");
    assert(-10n % 3n === -1n);
}
