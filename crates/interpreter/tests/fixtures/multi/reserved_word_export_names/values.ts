const x = 1;

// An export name is a module export name, not a binding, so strict mode's
// reserved words are allowed there, as in tsc.
export { x as static };
