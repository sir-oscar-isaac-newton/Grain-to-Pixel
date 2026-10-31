import type { SearchQuery } from '../../shared/ipc';

/** What the search bar and its filters hold, as typed. */
export interface Filters {
  text: string;
  /** Ids of the tags a frame must have. */
  tagIds: string[];
  film: string;
  camera: string;
  /** Digits only. */
  iso: string;
  from: string;
  to: string;
}

export const NO_FILTERS: Filters = {
  text: '',
  tagIds: [],
  film: '',
  camera: '',
  iso: '',
  from: '',
  to: '',
};

/** True when `filters` narrow the frames down at all. */
export function isFiltered(filters: Filters): boolean {
  return (
    filters.text.trim() !== '' ||
    filters.tagIds.length > 0 ||
    filters.film.trim() !== '' ||
    filters.camera.trim() !== '' ||
    filters.iso !== '' ||
    filters.from.trim() !== '' ||
    filters.to.trim() !== ''
  );
}

/** How many of the filters under the search box (not the text) are set. */
export function activeFilterCount(filters: Filters): number {
  return [
    filters.tagIds.length > 0,
    filters.film.trim() !== '',
    filters.camera.trim() !== '',
    filters.iso !== '',
    filters.from.trim() !== '',
    filters.to.trim() !== '',
  ].filter(Boolean).length;
}

const blank = (value: string): string | null => value.trim() || null;

/** The backend query for the filters, limited to a roll when one is chosen. */
export function toQuery(filters: Filters, rollId: string | null): SearchQuery {
  const iso = /^\d+$/.test(filters.iso) ? Number(filters.iso) : null;
  return {
    text: blank(filters.text),
    tags_all: filters.tagIds,
    roll_id: rollId,
    date_from: blank(filters.from),
    date_to: blank(filters.to),
    film_stock: blank(filters.film),
    camera: blank(filters.camera),
    iso: iso && iso > 0 ? iso : null,
  };
}
