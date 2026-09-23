// @target: es2015
const a: { x: () => void } = null as unknown as ({ x: () => void });
const b: { x?: () => void } = null as unknown as ({ x?: () => void });

const c = { ...a, ...b };


function main(): void {}
