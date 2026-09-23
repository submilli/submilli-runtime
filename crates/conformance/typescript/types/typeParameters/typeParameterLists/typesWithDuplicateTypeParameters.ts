// @target: es2015
class C<T, T> { }
class C2<T, U, T> { }

interface I<T, T> { }
interface I2<T, U, T> { }

function f<T, T>(): void { }
function f2<T, U, T>(): void { }

function main(): void {}
