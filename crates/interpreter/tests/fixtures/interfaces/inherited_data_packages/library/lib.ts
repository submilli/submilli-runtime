/** Generic data inherited by consumers. */
export interface Base<T> {
    /** Inherited value. */
    inherited: T;
    /** Optional inherited value. */
    optional?: T;
}
/** Derived data used directly by consumers. */
export interface Published extends Base<number> {
    /** Own value. */
    own: number;
}
