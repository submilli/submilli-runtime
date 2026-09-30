// @strict: true
// @target: es5, es2015
let a = () => {
    let arg = arguments[0];  // error
}

let b = function () {
    let a = () => {
        let arg = arguments[0];  // error
    }
}

function baz(): void {
	() => {
		let arg = arguments[0];
	}
}

function foo(inputFunc: () => void): void { }
foo(() => {
    let arg = arguments[0];  // error
});

function bar(): void {
    let arg = arguments[0];  // no error
}


() => {
	function foo(): void {
		let arg = arguments[0];  // no error
	}
}

function main(): void {}
