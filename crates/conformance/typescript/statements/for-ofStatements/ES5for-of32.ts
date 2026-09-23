//@target: ES5, ES2015

let array = [1,2,3];
let sum = 0;

for (let num of array) {
    if (sum === 0) {
        array = [4,5,6]
    }
    
    sum += num;
}

function main(): void {}
