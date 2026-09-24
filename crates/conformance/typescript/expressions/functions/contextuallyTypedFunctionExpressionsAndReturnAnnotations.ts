// @target: es2015
// @strict: true
function foo(x: (y: string) => (y2: number) => void): void { }

// Contextually type the parameter even if there is a return annotation
foo((y): (y2: number) => void => {
    let z = y.charAt(0); // Should be string
    return null;
});

foo((y: string) => {
    return y2 => {
        let z = y2.toFixed(); // Should be string
        return 0;
    };
});

function main(): void {}
