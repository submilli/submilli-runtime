// expect-error: cannot be used to index type
class Hidden { private secret: number = 1; visible: number = 2; }
function read<K extends "secret">(object: Hidden, key: K): number { return object[key]; }
function main(): void {}
