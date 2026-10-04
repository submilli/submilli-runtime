import { readdir } from 'node:fs/promises';

const previewDirectory = new URL('../public/_video-preview/', import.meta.url);
const files = await readdir(previewDirectory).catch((error) => {
  if (error.code === 'ENOENT') return [];
  throw error;
});
if (files.length) {
  throw new Error('Remove public/_video-preview/ media before building. Local review assets must not be published.');
}
