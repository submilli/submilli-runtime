// expect-error: expected `Other<
interface Box<X> { value: X }
interface Other<X> { value: X }
function main(): void {
    const source: Box<Box<Box<Box<Box<number>>>>> = { value: { value: { value: { value: { value: 1 } } } } };
    const target: Other<Other<Other<Other<Other<string>>>>> = source;
}
