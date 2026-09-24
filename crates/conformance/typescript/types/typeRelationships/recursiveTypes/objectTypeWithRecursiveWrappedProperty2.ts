// @target: es2015
// Basic recursive type

class List<T> {
    data: T;
    next: List<List<T>>;
}

let list1 = new List<number>();
let list2 = new List<number>();
let list3 = new List<string>();

list1 = list2; // ok
list1 = list3; // error

function main(): void {}
