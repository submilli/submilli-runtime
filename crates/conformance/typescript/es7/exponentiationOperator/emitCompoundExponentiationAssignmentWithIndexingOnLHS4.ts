// @target: es5, es2015

let globalCounter = 0;
function incrementIdx(max: number): number {
    globalCounter += 1;
    let idx = Math.floor(Math.random() * max);
    return idx;
}

let array1 = [1, 2, 3, 4, 5];

array1[incrementIdx(array1.length)] **= 3;

array1[incrementIdx(array1.length)] **= array1[incrementIdx(array1.length)] **= 2;

array1[incrementIdx(array1.length)] **= array1[incrementIdx(array1.length)] ** 2;

function main(): void {}
