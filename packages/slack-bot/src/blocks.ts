import { SlackError } from "./error";

/**
 * Attach nonempty Block Kit blocks without interpreting their contents.
 * @param body Message request body that may carry blocks.
 * @param blocks Blocks to send, or `undefined` when omitted.
 */
export function attachMessageBlocks(
    body: { blocks?: Record<string, unknown>[] },
    blocks: Record<string, unknown>[] | undefined,
): void {
    if (blocks === undefined || blocks.length === 0) return;
    if (blocks.length > 50) {
        throw new SlackError("invalid_blocks", "Slack messages support at most 50 blocks", 0);
    }
    body.blocks = blocks;
}
