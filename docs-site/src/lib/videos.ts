import registry from '../data/videos.json' with { type: 'json' };

export const films = registry.films;
const selectedIntroduction = films.find((film) => film.id === 'code-execution-introduction');
if (!selectedIntroduction) throw new Error('The code-execution introduction must be in the video registry');
export const introduction = selectedIntroduction;

// Local development may preview the approved file without making it a build asset.
export function videoSource(
  film: { status: string; src?: string | null },
  development = false,
  previewPath = '',
): string | undefined {
  if (development && /^\/docs\/_video-preview\/[a-zA-Z0-9_.-]+\.mp4$/.test(previewPath)) {
    return previewPath;
  }
  if (film.status === 'published' && film.src && /^https:\/\//.test(film.src)) return film.src;
  return undefined;
}
