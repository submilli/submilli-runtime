import registry from '../data/videos.json' with { type: 'json' };

export const films = registry.films;
export type Film = (typeof films)[number];
const selectedIntroduction = films.find((film) => film.id === 'code-execution-introduction');
if (!selectedIntroduction) throw new Error('The code-execution introduction must be in the video registry');
export const introduction = selectedIntroduction;

export function embedUrl(film: Film): string {
  return film.embedUrl;
}
