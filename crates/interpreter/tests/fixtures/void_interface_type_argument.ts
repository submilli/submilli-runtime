interface Cell<T> { value: T }
function take(cell: Cell<void>): boolean { return cell.value === undefined; }
function main(): void { assert(take({ value: undefined }), "interface generic field accepts void"); }
