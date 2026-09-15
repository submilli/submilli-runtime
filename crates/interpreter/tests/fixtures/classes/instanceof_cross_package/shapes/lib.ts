export class Shape {}

// Same-shape siblings across the package boundary: identical layout, distinct
// nominal identity.
export class Circle extends Shape {}

export class Square extends Shape {}
