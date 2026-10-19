import { z } from 'astro/zod';

// Reader purpose is independent of a page's route or sidebar group.
export const docTypeSchema = z.enum(['tutorial', 'how-to', 'explanation', 'reference']);
