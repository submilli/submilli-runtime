import registry from '../data/videos.json' with { type: 'json' };

export const films = registry.films;
export type Film = (typeof films)[number];
const selectedChallenges = films.find((film) => film.id === 'challenges');
if (!selectedChallenges) throw new Error('Challenges must be in the video registry');
export const challenges = selectedChallenges;
const selectedIntroduction = films.find((film) => film.id === 'code-execution-introduction');
if (!selectedIntroduction) throw new Error('The code-execution introduction must be in the video registry');
export const introduction = selectedIntroduction;

// Local development may preview the approved file without making it a build asset.
export function videoSource(
  film: { status: string; src?: string | null; publishedVersion?: string | null },
  development = false,
  previewPath = '',
): string | undefined {
  if (development && /^\/docs\/_video-preview\/[a-zA-Z0-9_.-]+\.mp4$/.test(previewPath)) {
    return previewPath;
  }
  if (film.status !== 'published' || !film.src || !film.publishedVersion?.trim()) return undefined;
  try {
    const url = new URL(film.src);
    const host = url.hostname;
    const localHost = host === 'localhost' || host.endsWith('.localhost') || host.endsWith('.local') || host === '[::1]'
      || host === '[::]' || host.startsWith('[::ffff:') || /^\[(?:f[cd][0-9a-f]{2}:|fe[89ab][0-9a-f]:)/i.test(host)
      || /^(0\.|127\.|10\.|255\.255\.255\.255$|192\.168\.|169\.254\.|172\.(1[6-9]|2\d|3[01])\.)/.test(host);
    if (url.protocol !== 'https:' || url.username || url.password || localHost) return undefined;
    return film.src;
  } catch {
    return undefined;
  }
}

export function previewPath(film: Film, introductionPath = '', challengesPath = ''): string {
  if (film.id === 'code-execution-introduction') return introductionPath;
  if (film.id === 'challenges') return challengesPath;
  return '';
}
